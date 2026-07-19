/**
 * upload.js — CSV / JSONL file parser → batch product indexing.
 *
 * Usage:
 *   import { mountUploader } from './upload.js';
 *   mountUploader('upload-section', client, indexName, onDone);
 */

/**
 * Mount the file drop zone and upload controls into `containerId`.
 *
 * @param {string}         containerId  DOM element to mount into
 * @param {VectoriaClient} client       Authenticated API client
 * @param {string|null}    indexName    Target index (null → default)
 * @param {function}       onDone       Called with { indexed, errors } when complete
 */
export function mountUploader(containerId, client, indexName, onDone) {
  const container = document.getElementById(containerId);
  if (!container) return;

  // Build UI with DOM methods
  const dropZone = document.createElement('div');
  dropZone.className = 'drop-zone';
  dropZone.setAttribute('tabindex', '0');
  dropZone.setAttribute('role', 'button');
  dropZone.setAttribute('aria-label', 'Drop a CSV or JSONL file here, or click to browse');

  const dropLabel = document.createElement('p');
  dropLabel.textContent = 'Drop a CSV or JSONL file here, or click to browse';
  const dropSub = document.createElement('p');
  dropSub.className = 'text-muted mt-1';
  dropSub.textContent = 'Each row/line becomes one product. Fields: id, text, and any metadata columns.';
  dropZone.append(dropLabel, dropSub);

  const fileInput = document.createElement('input');
  fileInput.type = 'file';
  fileInput.accept = '.csv,.jsonl,.ndjson,.json';
  fileInput.style.display = 'none';

  const statusEl = document.createElement('div');
  statusEl.className = 'mt-2 text-muted';

  const progressWrap = document.createElement('div');
  progressWrap.className = 'progress-bar mt-1';
  progressWrap.style.display = 'none';
  const progressFill = document.createElement('div');
  progressFill.className = 'progress-fill';
  progressFill.style.width = '0%';
  progressWrap.appendChild(progressFill);

  const concurrencyLabel = document.createElement('label');
  concurrencyLabel.textContent = 'Parallel uploads';
  concurrencyLabel.className = 'mt-2';
  concurrencyLabel.style.display = 'block';
  concurrencyLabel.style.fontSize = '0.8rem';
  concurrencyLabel.style.color = 'var(--muted)';

  const concurrencyInput = document.createElement('input');
  concurrencyInput.type = 'number';
  concurrencyInput.min = '1';
  concurrencyInput.max = '20';
  concurrencyInput.value = '5';
  concurrencyInput.style.width = '80px';
  concurrencyInput.setAttribute('aria-label', 'Parallel uploads');

  container.append(dropZone, fileInput, concurrencyLabel, concurrencyInput, statusEl, progressWrap);

  // Event wiring
  dropZone.addEventListener('click', () => fileInput.click());
  dropZone.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') fileInput.click(); });

  dropZone.addEventListener('dragover', e => { e.preventDefault(); dropZone.classList.add('drag-over'); });
  dropZone.addEventListener('dragleave', () => dropZone.classList.remove('drag-over'));
  dropZone.addEventListener('drop', e => {
    e.preventDefault();
    dropZone.classList.remove('drag-over');
    const file = e.dataTransfer?.files?.[0];
    if (file) processFile(file);
  });
  fileInput.addEventListener('change', () => {
    if (fileInput.files?.[0]) processFile(fileInput.files[0]);
  });

  async function processFile(file) {
    statusEl.textContent = `Parsing ${file.name}…`;
    progressWrap.style.display = 'none';
    progressFill.style.width = '0%';

    let products;
    try {
      const text = await readFileAsText(file);
      if (file.name.endsWith('.csv')) {
        products = parseCSV(text);
      } else {
        products = parseJSONL(text);
      }
    } catch (e) {
      statusEl.textContent = `Parse error: ${e.message}`;
      statusEl.className = 'mt-2 text-danger';
      return;
    }

    if (!products.length) {
      statusEl.textContent = 'File is empty or could not be parsed.';
      statusEl.className = 'mt-2 text-danger';
      return;
    }

    statusEl.textContent = `Uploading ${products.length} products…`;
    statusEl.className = 'mt-2 text-muted';
    progressWrap.style.display = 'block';

    const concurrency = Math.max(1, Math.min(20, parseInt(concurrencyInput.value, 10) || 5));

    const result = await client.indexProductBatch(products, indexName, {
      concurrency,
      onProgress(done, total) {
        const pct = Math.round((done / total) * 100);
        progressFill.style.width = `${pct}%`;
        statusEl.textContent = `Uploading… ${done}/${total}`;
      },
    });

    progressFill.style.width = '100%';
    if (result.errors.length === 0) {
      statusEl.textContent = `Done — ${result.indexed} products indexed.`;
      statusEl.className = 'mt-2 text-ok';
    } else {
      statusEl.textContent = `${result.indexed} indexed, ${result.errors.length} failed. Check console for details.`;
      statusEl.className = 'mt-2 text-danger';
      console.warn('Upload errors:', result.errors);
    }

    onDone?.(result);
    // Reset file input so same file can be re-uploaded
    fileInput.value = '';
  }
}

// ── Parsers ──────────────────────────────────────────────────────────────────

function readFileAsText(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = e => resolve(e.target.result);
    reader.onerror = () => reject(new Error('Could not read file'));
    reader.readAsText(file);
  });
}

/**
 * Parse JSONL / NDJSON. Each non-empty line must be a JSON object.
 * Products must have `id` and `text` fields; anything else lands in `metadata`.
 */
function parseJSONL(text) {
  return text
    .split('\n')
    .map(l => l.trim())
    .filter(Boolean)
    .map((line, i) => {
      let obj;
      try { obj = JSON.parse(line); }
      catch { throw new Error(`Line ${i + 1}: invalid JSON`); }
      return normalizeProduct(obj, i);
    });
}

/**
 * Parse CSV with a header row.
 * Handles quoted fields (RFC 4180 subset: double-quoted, commas inside quotes).
 */
function parseCSV(text) {
  const lines = text.split(/\r?\n/).map(l => l.trim()).filter(Boolean);
  if (lines.length < 2) throw new Error('CSV must have a header row and at least one data row');

  const headers = splitCSVLine(lines[0]);
  return lines.slice(1).map((line, i) => {
    const values = splitCSVLine(line);
    const obj = {};
    headers.forEach((h, j) => { obj[h.trim()] = values[j]?.trim() ?? ''; });
    return normalizeProduct(obj, i);
  });
}

function splitCSVLine(line) {
  const cells = []; let cur = ''; let inQuote = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === '"') {
      if (inQuote && line[i + 1] === '"') { cur += '"'; i++; }
      else inQuote = !inQuote;
    } else if (ch === ',' && !inQuote) {
      cells.push(cur); cur = '';
    } else {
      cur += ch;
    }
  }
  cells.push(cur);
  return cells;
}

/**
 * Normalize a raw parsed row into { id, text, metadata }.
 * Accepts common ID column names: id, sku, product_id, asin.
 */
function normalizeProduct(obj, index) {
  const id   = obj.id ?? obj.sku ?? obj.product_id ?? obj.asin ?? `row-${index + 1}`;
  const text = obj.text ?? obj.title ?? obj.name ?? obj.description ?? '';
  const metadata = {};
  for (const [k, v] of Object.entries(obj)) {
    if (!['id','sku','product_id','asin','text','title','name','description'].includes(k)) {
      metadata[k] = v;
    }
  }
  return { id: String(id), text: String(text), metadata };
}

/**
 * nav.js — shared navigation header and session bootstrap.
 *
 * Call mountNav(containerId, activePage) once per page.
 * Session must already be set (via session.save in api.js) before mounting.
 */
import { session, VectoriaClient } from './api.js';

const PAGES = {
  admin:     { href: 'admin.html',     label: 'Tenants & Indexes',  roles: ['admin'] },
  tenant:    { href: 'tenant.html',    label: 'Search & Overrides', roles: ['admin', 'tenant'] },
  api_guide: { href: 'api-guide.html', label: 'API Guide',          roles: ['admin', 'tenant'] },
};

/** Mount the nav bar into `containerId`. `activePage` is 'admin' or 'tenant'. */
export async function mountNav(containerId, activePage) {
  const s = session.load();
  if (!s.url || !s.key) {
    redirect('index.html');
    return;
  }

  const container = document.getElementById(containerId);
  if (!container) return;

  const roleTag = s.role === 'admin' ? 'admin' : (s.index || 'tenant');

  // Build nav with DOM methods — no innerHTML with dynamic content.
  const nav = document.createElement('nav');

  const brand = document.createElement('span');
  brand.className = 'brand';
  brand.textContent = 'vectoria ';
  const brandSub = document.createElement('span');
  brandSub.textContent = 'console';
  brand.appendChild(brandSub);
  nav.appendChild(brand);

  const linksDiv = document.createElement('div');
  linksDiv.className = 'links';
  Object.entries(PAGES)
    .filter(([, p]) => p.roles.includes(s.role))
    .forEach(([key, p]) => {
      const a = document.createElement('a');
      a.href = p.href;
      a.textContent = p.label;
      if (key === activePage) a.classList.add('active');
      linksDiv.appendChild(a);
    });
  nav.appendChild(linksDiv);

  const statusDiv = document.createElement('div');
  statusDiv.className = 'status';

  const dot = document.createElement('span');
  dot.className = 'dot';
  dot.id = 'health-dot';

  const label = document.createElement('span');
  label.id = 'health-label';
  label.textContent = 'connecting…';

  const roleSpan = document.createElement('span');
  roleSpan.className = 'role-tag';
  roleSpan.textContent = roleTag;

  const logoutBtn = document.createElement('button');
  logoutBtn.className = 'btn btn-ghost btn-sm';
  logoutBtn.textContent = 'logout';
  logoutBtn.id = 'logout-btn';

  statusDiv.append(dot, label, roleSpan, logoutBtn);
  nav.appendChild(statusDiv);
  container.appendChild(nav);

  logoutBtn.addEventListener('click', () => {
    session.clear();
    redirect('index.html');
  });

  pingHealth(s.url, s.key);
}

async function pingHealth(url, key) {
  try {
    const client = new VectoriaClient(url, key);
    const data = await client.health();
    setDot('ok', `v${data.version || '?'}`);
  } catch {
    setDot('err', 'offline');
  }
}

function setDot(cls, label) {
  const dot = document.getElementById('health-dot');
  const lbl = document.getElementById('health-label');
  if (dot) dot.className = `dot ${cls}`;
  if (lbl) lbl.textContent = label;
}

export function redirect(href) {
  window.location.href = href;
}

/**
 * Escape a string for safe insertion into HTML attribute values or text nodes
 * when you must use innerHTML (e.g. for mixed-content table cells).
 * Prefer textContent / DOM methods wherever possible.
 */
export function escHtml(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/** Show flash message. type: 'ok' | 'err' */
export function flash(containerId, message, type = 'ok') {
  const el = document.getElementById(containerId);
  if (!el) return;
  el.textContent = message;
  el.className = `flash flash-${type} show`;
  setTimeout(() => el.classList.remove('show'), 3500);
}

/** Simple tab switcher. Call once per page. */
export function initTabs(tabsEl) {
  const btns = tabsEl.querySelectorAll('.tab-btn');
  btns.forEach(btn => {
    btn.addEventListener('click', () => {
      const target = btn.dataset.tab;
      btns.forEach(b => b.classList.toggle('active', b === btn));
      document.querySelectorAll('.tab-pane').forEach(p =>
        p.classList.toggle('active', p.id === target)
      );
    });
  });
  // Activate first tab on load
  if (btns.length) btns[0].click();
}

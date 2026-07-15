# vectoria-search

Smart search widget for [Vectoria](https://vectoriasearch.com). Connects to any Vectoria server and enhances queries client-side — synonyms, Unicode normalization, and pt-BR/en-US locale support — with **zero extra roundtrips**.

One HTTP request per search. All query improvement happens locally in JavaScript before the request is sent.

---

## Quick start

### Script tag (no build step)

```html
<script src="vectoria-search.js"></script>

<div id="search"></div>

<script>
VectoriaSearch.init({
  container: '#search',
  url: 'https://your-vectoria-server.com',
  apiKey: 'your-api-key',
  locale: 'en-US',         // or 'pt-BR'
});
</script>
```

### Web Component (works in React, Vue, Svelte, Angular, plain HTML)

```html
<script src="vectoria-search.js"></script>

<vectoria-search
  url="https://your-vectoria-server.com"
  api-key="your-api-key"
  locale="pt-BR"
  mode="hybrid"
  limit="10">
</vectoria-search>
```

### ES Module / npm

```bash
npm install vectoria-search
```

```javascript
import VectoriaSearch from 'vectoria-search';

VectoriaSearch.init({
  container: document.getElementById('search'),
  url: 'https://your-vectoria-server.com',
  apiKey: 'your-api-key',
  locale: 'pt-BR',
});
```

### React (via Web Component)

```jsx
import 'vectoria-search';

export function SearchBar({ onSelect }) {
  return (
    <vectoria-search
      url="https://your-server.com"
      api-key={process.env.REACT_APP_API_KEY}
      locale="en-US"
      onVsSelect={(e) => onSelect(e.detail)}
    />
  );
}
```

> React 18 and earlier pass custom-element events as `onVsSelect`. React 19+ supports standard DOM events natively on custom elements.

### Vue

```vue
<template>
  <vectoria-search
    :url="serverUrl"
    :api-key="apiKey"
    locale="pt-BR"
    @vs-select="handleSelect"
  />
</template>

<script setup>
import 'vectoria-search';
</script>
```

### Svelte

```svelte
<script>
  import 'vectoria-search';
  function handleSelect(e) { console.log(e.detail); }
</script>

<vectoria-search
  url="https://your-server.com"
  api-key="your-key"
  locale="en-US"
  on:vs-select={handleSelect}
/>
```

---

## Configuration

All options for `VectoriaSearch.init()` and their attribute equivalents:

| Option (JS)    | Attribute        | Default                          | Description                                           |
|----------------|------------------|----------------------------------|-------------------------------------------------------|
| `container`    | —                | `'#vectoria-search'`             | CSS selector or DOM element to mount into             |
| `url`          | `url`            | `'https://demo.vectoriasearch.com'` | Vectoria server base URL                           |
| `apiKey`       | `api-key`        | `'vectoria-esci-demo'`           | API key (Bearer token)                                |
| `locale`       | `locale`         | `'en-US'`                        | `'en-US'` or `'pt-BR'`                               |
| `synonyms`     | —                | `{}`                             | Custom synonym map, merged with built-in              |
| `mode`         | `mode`           | `'hybrid'`                       | `'hybrid'` \| `'bm25'` \| `'semantic'`               |
| `limit`        | `limit`          | `10`                             | Max results                                           |
| `indexName`    | `index-name`     | `null`                           | Named index (`/indexes/{name}/search`)                |
| `autocomplete` | `autocomplete`   | `true`                           | Show autocomplete dropdown (calls `/autocomplete`)    |
| `enhance`      | —                | `true`                           | Enable client-side query enhancement                  |
| `debounce`     | `debounce`       | `250`                            | Milliseconds to wait after last keystroke             |
| `placeholder`  | `placeholder`    | locale-specific                  | Override input placeholder text                       |
| `injectStyles` | —                | `true`                           | Inject default CSS into `<head>`                      |
| `renderHit`    | —                | built-in renderer                | `(hit) => string` — custom hit card HTML              |
| `onResults`    | —                | `null`                           | `(results, {original, enhanced}) => void`             |
| `onSelect`     | —                | `null`                           | `(hit) => void` called when a result is clicked       |
| `onError`      | —                | `null`                           | `(error) => string\|void` called on fetch failure     |

---

## Query enhancement

The widget enhances queries before sending them to Vectoria — without any additional HTTP requests.

### What it does

1. **Unicode normalization** — trims, lowercases, collapses whitespace.
2. **Accent variants (pt-BR)** — appends the accent-stripped form so BM25 matches both `tenis` and `tênis`.
3. **Synonym expansion** — appends related terms from the built-in dictionary. Example:
   - `tênis corrida` → sent as `tênis corrida sapato calçado sneaker shoes corrida running jogging esporte`
   - `wireless headphones` → sent as `wireless headphones bluetooth cordless earphone earbud headset audio`

The original query is shown to the user; the enhanced query is sent to Vectoria. The widget displays which expansions were added below the search box.

### Custom synonyms

Pass your own synonym map to merge with the built-in dictionary:

```javascript
VectoriaSearch.init({
  container: '#search',
  url: 'https://your-server.com',
  apiKey: 'key',
  locale: 'pt-BR',
  synonyms: {
    'chinelo': ['sandália', 'flip flop', 'havaianas'],
    'fone gamer': ['headset', 'gaming headset', 'headphone gamer'],
  },
});
```

### Disable enhancement

```javascript
VectoriaSearch.init({ ..., enhance: false });
```

### Use the enhancer standalone (no UI)

```javascript
const enh = VectoriaSearch.enhancer('pt-BR', { 'mochila': ['backpack', 'bag'] });
const { original, enhanced, expansions } = enh.enhance('mochila impermeável');
// enhanced = 'mochila impermeável backpack bag waterproof water resistant chuva'
await fetch('/search', { method: 'POST', body: JSON.stringify({ q: enhanced }) });
```

---

## Events (Web Component)

| Event        | `detail`                                     | When                           |
|--------------|----------------------------------------------|--------------------------------|
| `vs-results` | `{ hits, original, enhanced }`               | After each successful search   |
| `vs-select`  | `hit` object (id, score, metadata)           | User clicks a result card      |
| `vs-error`   | `Error`                                      | Network or server error        |

```javascript
const el = document.querySelector('vectoria-search');
el.addEventListener('vs-select', (e) => {
  window.location.href = `/products/${e.detail.id}`;
});
```

---

## Custom result renderer

```javascript
VectoriaSearch.init({
  container: '#search',
  url: 'https://your-server.com',
  apiKey: 'key',
  renderHit: (hit) => {
    const m = hit.metadata;
    // Return HTML string. You are responsible for escaping dynamic content.
    return `
      <strong>${escapeHtml(m.title)}</strong>
      <span>${escapeHtml(m.brand)} — $${m.price}</span>
    `;
  },
});
```

> **Security**: if you supply `renderHit`, escape all dynamic values yourself. The built-in renderer escapes all server-sourced fields.

---

## Theming (CSS custom properties)

Override any of these on `:root` or a parent element:

```css
#search {
  --vs-primary: #7c3aed;        /* focus ring + score badges */
  --vs-border:  #e2e8f0;        /* input and card border */
  --vs-border2: #cbd5e1;        /* hover border */
  --vs-bg:      #ffffff;        /* input and card background */
  --vs-bg2:     #f8fafc;        /* autocomplete hover */
  --vs-text:    #0f172a;        /* primary text */
  --vs-text2:   #334155;        /* secondary text */
  --vs-muted:   #64748b;        /* placeholder, meta */
  --vs-radius:  8px;            /* border radius */
  --vs-shadow:  0 4px 16px rgba(0,0,0,0.10);
  --vs-font:    'Inter', system-ui, sans-serif;
  --vs-mono:    'SF Mono', monospace;
}
```

---

## CORS

The widget makes `fetch()` calls from the browser to your Vectoria server. Your server must send CORS headers for the origin of the page embedding the widget.

Add to your Vectoria deployment (e.g. via a reverse proxy or `vectoria.toml`):

```
Access-Control-Allow-Origin: https://your-site.com
Access-Control-Allow-Headers: Authorization, Content-Type
Access-Control-Allow-Methods: GET, POST
```

For development with `localhost`, use `Access-Control-Allow-Origin: *`.

---

## Runtime API

```javascript
const widget = VectoriaSearch.init({ ... });

widget.search('running shoes');   // programmatic search
widget.setOption('locale', 'pt-BR');  // change locale at runtime
widget.setOption('mode', 'semantic'); // change mode at runtime
widget.clear();                   // clear input and results
widget.destroy();                 // remove widget from DOM
```

---

## Browser support

`replaceChildren()`, Web Components, `fetch`, and `customElements` — all supported in Chrome 86+, Firefox 78+, Safari 14.1+. For older browsers, a polyfill for `replaceChildren` (`el.innerHTML = ''`) is sufficient.

---

## License

MIT

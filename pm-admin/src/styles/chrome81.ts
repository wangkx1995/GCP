import type { Transformer, CSSObject } from '@ant-design/cssinjs';

function isPlainObject(val: unknown): val is Record<string, unknown> {
  return val !== null && typeof val === 'object' && !Array.isArray(val) && val.constructor === Object;
}

function isFlexDisplay(val: unknown): boolean {
  return val === 'flex' || val === 'inline-flex';
}

function replaceDeep(obj: CSSObject, keyFn: (k: string) => string): CSSObject {
  const out: CSSObject = {};
  for (let [key, val] of Object.entries(obj)) {
    key = keyFn(key);
    out[key] = isPlainObject(val) ? replaceDeep(val as CSSObject, keyFn) : val;
  }
  return out;
}

export const focusVisibleTransformer: Transformer = {
  visit: (cssObj) => replaceDeep(cssObj, k => k.replace(/:focus-visible/g, ':focus')),
};

function getGapValue(val: unknown): string | null {
  if (typeof val === 'string' && /^\d+px$/.test(val)) return val;
  if (typeof val === 'number') return `${val}px`;
  return null;
}

export const flexGapTransformer: Transformer = {
  visit: (cssObj) => {
    const clone: CSSObject = {};
    const keys = Object.keys(cssObj);
    const display = cssObj.display;
    const gap = cssObj.gap;

    if (isFlexDisplay(display) && gap !== undefined) {
      for (const key of keys) {
        if (key === 'gap') continue;
        if (key.startsWith('@supports')) continue;
        clone[key] = cssObj[key];
      }

      const gapPx = getGapValue(gap);
      if (gapPx) {
        clone['@supports (gap: 1px)'] = { gap };
        clone['@supports not (gap: 1px)'] = {
          '> *': {
            marginBlockEnd: gapPx,
            marginInlineEnd: gapPx,
            '&:last-child': { marginBlockEnd: 0, marginInlineEnd: 0 },
          },
        };
      } else {
        clone.gap = gap;
      }
    } else {
      Object.assign(clone, cssObj);
    }

    return clone;
  },
};

export function detectFlexGapSupport(): boolean {
  if (typeof document === 'undefined') return true;
  const el = document.createElement('div');
  el.style.cssText = 'display:flex;gap:1px;position:absolute;visibility:hidden';
  document.body.appendChild(el);
  const supported = getComputedStyle(el).gap === '1px';
  document.body.removeChild(el);
  return supported;
}

export function applyFlexGapPolyfill(root: HTMLElement = document.body): () => void {
  if (detectFlexGapSupport()) return () => {};

  const style = document.createElement('style');
  style.id = '__flex-gap-polyfill';
  style.textContent = `
    [data-gap-fallback] > * + * {
      margin-block-start: var(--gap-fb-block, 0) !important;
      margin-inline-start: var(--gap-fb-inline, 0) !important;
    }
    [data-gap-fallback] > *:last-child {
      margin-block-start: 0 !important;
      margin-inline-start: 0 !important;
    }
  `;
  document.head.appendChild(style);

  function processElement(el: Element) {
    if (el.getAttribute('data-gap-fallback') === 'true') return;
    const cs = getComputedStyle(el);
    if (!isFlexDisplay(cs.display)) return;
    const dispGap = cs.gap;
    if (!dispGap || dispGap === 'normal') return;
    const parts = dispGap.split(' ');
    const rowGap = parseFloat(parts[0] || '0');
    const colGap = parseFloat(parts[1] || parts[0] || '0');
    if (!rowGap && !colGap) return;

    const fd = cs.flexDirection;
    const isRow = fd === 'row' || fd === 'row-reverse';
    el.setAttribute('data-gap-fallback', 'true');
    (el as HTMLElement).style.setProperty('--gap-fb-block', isRow ? `${rowGap}px` : `${colGap}px`);
    (el as HTMLElement).style.setProperty('--gap-fb-inline', isRow ? `${colGap}px` : `${rowGap}px`);
  }

  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
  let node: Node | null;
  while ((node = walker.nextNode())) processElement(node as Element);

  const observer = new MutationObserver((mutations) => {
    for (const m of mutations) {
      for (const n of m.addedNodes) {
        if (n instanceof Element) {
          if (n.matches('[style*="display"]') || n.matches('[class*="ant-"]')) processElement(n);
          if (n.querySelectorAll) n.querySelectorAll('[class*="ant-"]').forEach(processElement);
        }
      }
    }
  });
  observer.observe(root, { childList: true, subtree: true });

  return () => observer.disconnect();
}

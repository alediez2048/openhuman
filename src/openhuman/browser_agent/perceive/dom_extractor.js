// F3-2 — DOM extractor.
//
// Runs once per `snapshot()` via `Runtime.evaluate`. Walks the
// document for actionable elements, computes their bounds + accessible
// name + xpath, and returns a single JSON payload the Rust side
// parses into `Vec<ActionableElement>`.
//
// Constraints from CLAUDE.md:
//   • One-off agent-driven `evaluate` only — NEVER persisted via
//     `addScriptToEvaluateOnNewDocument`.
//   • Scoped to the active session; not added to init scripts.
//
// Size budget: <8 KB minified. Inline-bundled via `include_str!`.
//
// Output shape (consumed by `snapshot.rs::parse_dom_extractor_output`):
//   {
//     "url": "...",
//     "title": "...",
//     "viewport": { "width": …, "height": …, "device_pixel_ratio": … },
//     "text_excerpt": "…",
//     "elements": [
//       {
//         "tag": "button",
//         "role_hint": "button"|null,
//         "label": "Save",
//         "bounds": { "x": …, "y": …, "width": …, "height": … },
//         "xpath": "/html/body/.../button[1]",
//         "disabled": false,
//         "checked": false,
//         "expanded": false,
//         "focused": false,
//         "hidden": false,
//         "attrs": { "type": "submit", "name": "save", "aria-label": "Save changes" }
//       },
//       …
//     ]
//   }

(function () {
  function xpathOf(node) {
    if (node === document.body) return "/html/body";
    if (!node || !node.parentNode) return "";
    var idx = 1;
    var sib = node.previousElementSibling;
    while (sib) {
      if (sib.nodeName === node.nodeName) idx += 1;
      sib = sib.previousElementSibling;
    }
    var tag = node.nodeName.toLowerCase();
    return xpathOf(node.parentNode) + "/" + tag + "[" + idx + "]";
  }

  function isVisible(el, rect) {
    if (rect.width === 0 || rect.height === 0) return false;
    var style = window.getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden") return false;
    if (parseFloat(style.opacity || "1") === 0) return false;
    // Off-screen: anything entirely outside the viewport.
    if (rect.bottom < 0 || rect.right < 0) return false;
    if (rect.top > window.innerHeight + 5000) return false; // far below = hidden
    return true;
  }

  function accessibleName(el) {
    var aria = el.getAttribute("aria-label");
    if (aria && aria.trim()) return aria.trim();
    var labelledby = el.getAttribute("aria-labelledby");
    if (labelledby) {
      var refs = labelledby
        .split(/\s+/)
        .map(function (id) {
          var n = document.getElementById(id);
          return n ? n.textContent.trim() : "";
        })
        .filter(Boolean);
      if (refs.length) return refs.join(" ");
    }
    var text = (el.textContent || "").replace(/\s+/g, " ").trim();
    if (text) return text.slice(0, 200);
    var placeholder = el.getAttribute("placeholder");
    if (placeholder) return placeholder;
    var value = el.value;
    if (value) return String(value).slice(0, 200);
    var title = el.getAttribute("title");
    if (title) return title;
    return "";
  }

  function pickAttrs(el) {
    var out = {};
    var wanted = ["type", "name", "value", "href", "placeholder", "src", "alt"];
    for (var i = 0; i < wanted.length; i++) {
      var v = el.getAttribute(wanted[i]);
      if (v != null) out[wanted[i]] = String(v).slice(0, 200);
    }
    // Pull aria-* attributes (narrow set, not all of them).
    var ariaWanted = ["aria-label", "aria-expanded", "aria-checked", "aria-selected", "aria-disabled"];
    for (var j = 0; j < ariaWanted.length; j++) {
      var av = el.getAttribute(ariaWanted[j]);
      if (av != null) out[ariaWanted[j]] = String(av).slice(0, 200);
    }
    return out;
  }

  function ariaBool(el, attr) {
    var v = el.getAttribute(attr);
    return v === "true";
  }

  var selectors = [
    "a[href]",
    "button",
    "input:not([type=hidden])",
    "select",
    "textarea",
    "[role]",
    "[onclick]",
    "[tabindex]:not([tabindex=\"-1\"])",
    "iframe",
    "h1", "h2", "h3"
  ].join(",");

  var nodes = Array.prototype.slice.call(document.querySelectorAll(selectors));
  // Deduplicate — querySelectorAll with this OR list can match the
  // same element via multiple selectors (e.g. `<button role="button">`).
  var seen = new Set();
  var elements = [];

  for (var i = 0; i < nodes.length; i++) {
    var el = nodes[i];
    if (seen.has(el)) continue;
    seen.add(el);

    var rect = el.getBoundingClientRect();
    if (!isVisible(el, rect)) continue;

    var tag = el.nodeName.toLowerCase();
    var roleHint = el.getAttribute("role");

    var rec = {
      tag: tag,
      role_hint: roleHint,
      label: accessibleName(el),
      bounds: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      xpath: xpathOf(el),
      disabled: !!el.disabled || ariaBool(el, "aria-disabled"),
      checked: !!el.checked || ariaBool(el, "aria-checked"),
      expanded: ariaBool(el, "aria-expanded"),
      focused: document.activeElement === el,
      hidden: el.hidden,
      attrs: pickAttrs(el)
    };

    if (tag === "iframe") {
      rec.attrs.src = rec.attrs.src || el.src || "";
    }

    elements.push(rec);
  }

  // Trimmed page text — first 1500 chars of <main>, falling back to
  // body. The token-budget renderer trims further.
  var textRoot = document.querySelector("main") || document.body;
  var textExcerpt = "";
  if (textRoot) {
    textExcerpt = (textRoot.innerText || "").replace(/\s+/g, " ").trim().slice(0, 1500);
  }

  return {
    url: location.href,
    title: document.title || "",
    viewport: {
      width: window.innerWidth,
      height: window.innerHeight,
      device_pixel_ratio: window.devicePixelRatio || 1
    },
    text_excerpt: textExcerpt,
    elements: elements
  };
})();

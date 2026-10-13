/* Dreamy interface logic. Plain JavaScript, no build step. */
(() => {
  "use strict";

  /* ==========================================================================
     Backend bridge. Inside the desktop app this talks to Rust. In a plain
     browser (for previewing the design) it uses an in-memory stand-in.
     ========================================================================== */
  const preview = { dreams: {}, tags: "[]" };
  const invoke = window.__TAURI__
    ? window.__TAURI__.core.invoke
    : async (cmd, args = {}) => {
        if (cmd === "save_dream") {
          const stem = args.filename.replace(/\.md$/, "");
          let name = args.filename;
          for (let n = 2; name in preview.dreams; n++) name = stem + "_" + n + ".md";
          preview.dreams[name] = args.content;
          return name;
        }
        if (cmd === "list_dreams") return Object.keys(preview.dreams).sort().reverse();
        if (cmd === "read_dreams") return Object.keys(preview.dreams).sort().reverse().map((name) => [name, preview.dreams[name]]);
        if (cmd === "read_dream") {
          if (!(args.filename in preview.dreams)) throw new Error("Not found: " + args.filename);
          return preview.dreams[args.filename];
        }
        if (cmd === "load_tags") return preview.tags;
        if (cmd === "save_tags") { preview.tags = args.content; return null; }
        throw new Error("Unknown command: " + cmd);
      };

  /* Small saved preferences (theme, motion). Failure is harmless. */
  const prefs = {
    get(key, fallback) {
      try {
        const value = localStorage.getItem("dreamy." + key);
        return value === null ? fallback : JSON.parse(value);
      } catch (err) {
        return fallback;
      }
    },
    set(key, value) {
      try { localStorage.setItem("dreamy." + key, JSON.stringify(value)); } catch (err) { /* ignore */ }
    },
  };

  /* ==========================================================================
     Constants
     ========================================================================== */
  const LEVELS = ["Not lucid", "Briefly aware", "Partly lucid", "Lucid", "Fully lucid, in control"];

  const COLORS = ["#a897ff", "#86d6ff", "#f08aa8", "#f5b768", "#5fd6c0", "#9fd66f", "#d68ae0", "#eee9ff"];

  // Each symbol is one drawn path (24 x 24 grid, stroked)
  const SYMBOLS = {
    moon: "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z",
    star: "M12 3l2.6 5.6 6 .7-4.5 4.1 1.2 6L12 16.9 6.7 19.4l1.2-6L3.4 9.3l6-.7z",
    eye: "M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12zm7 0a3 3 0 1 0 6 0 3 3 0 1 0-6 0z",
    key: "M12 12a4 4 0 1 1-8 0 4 4 0 1 1 8 0zM12 12h9M18 12v3M21 12v2",
    door: "M6 21V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v17M3 21h18M14.5 12h.01",
    wave: "M2 9c2.5-3 5-3 7.5 0s5 3 7.5 0 3-2 5-1.2M2 15.5c2.5-3 5-3 7.5 0s5 3 7.5 0 3-2 5-1.2",
    feather: "M19 4C11 4 7 9 6 20M6 20c2-4 5-6 9-8M10 14c2 0 4-1 6-3",
    flame: "M12 21c4 0 6.5-2.6 6.5-6.5 0-3.6-2.4-5.4-3.6-8.5-1.1 1.7-1.6 2.9-1.8 4-.9-1.2-1.4-2.6-1.2-4.4C8.6 8.8 5.5 11.4 5.5 14.5 5.5 18.4 8 21 12 21z",
    leaf: "M5 19C5 10 10 5 19 5c0 9-5 14-14 14zM5 19l7-7",
    cloud: "M7 18.5a4.5 4.5 0 0 1-.7-8.9A6 6 0 0 1 18 9.6a4 4 0 0 1 .4 8.9z",
    spiral: "M12 12a2 2 0 1 1 2 2 4 4 0 1 1-4-4 6 6 0 1 1 6 6",
    bolt: "M13 2.5 4.5 14H11l-1 7.5L18.5 10H13z",
  };

  // Sky colours through the day for the Dawn world: [hour, top, bottom]
  const SKY_STOPS = [
    [0, "#0d0a22", "#241640"],
    [5, "#1d1540", "#4a2a55"],
    [6.5, "#2e2050", "#9c6472"],
    [8, "#1f1a46", "#5b3a6a"],
    [17, "#22173f", "#4d2f5e"],
    [19, "#2c1638", "#8a4a5e"],
    [20.5, "#170f30", "#3a2350"],
    [24, "#0d0a22", "#241640"],
  ];

  const THEMES = [
    { id: "night", name: "Deep night sky", desc: "A slow starfield under moonlight. Quiet and nocturnal.", preview: "pv-night" },
    { id: "dawn", name: "Dawn fading", desc: "Dusk to rose-gold. The sky follows the time of day.", preview: "pv-dawn" },
    { id: "lucid", name: "Lucid instrument", desc: "Thin lines, a scanning light, and a clinical edge.", preview: "pv-lucid" },
  ];

  /* ==========================================================================
     State and elements
     ========================================================================== */
  const state = {
    theme: "night",
    motion: true,
    level: 0,
    library: [],      // every tag: { name, color, symbol }
    selected: [],     // tag names on the current dream, in order
    designer: null,   // open tag designer: { editing, color, symbol }
    dreams: [],       // saved dream file names, newest first
    showAll: false,   // recent list: first ten, or every dream
    journal: [],      // every saved dream, parsed: { name, lucidity, tags, body }
    filters: { query: "", sort: "newest", lucidity: "any", tags: [] }, // Dreams sidebar
    dreamsOpen: false, // Dreams panel expanded beside the rail
  };

  const RECENT_LIMIT = 10;

  const $ = (selector) => document.querySelector(selector);
  const root = document.documentElement;
  const body = document.body;

  const dreamTime = $("#dreamTime");
  const dreamEl = $("#dream");
  const orbsEl = $("#orbs");
  const captionEl = $("#caption");
  const pillsEl = $("#pills");
  const tagInput = $("#tagInput");
  const tagfieldEl = $("#tagfield");
  const libraryEl = $("#library");
  const designBtn = $("#designBtn");
  const designerEl = $("#designer");
  const designerTitle = $("#designerTitle");
  const designerPreview = $("#designerPreview");
  const designerName = $("#designerName");
  const designerError = $("#designerError");
  const designerSave = $("#designerSave");
  const swatchesEl = $("#swatches");
  const symbolsEl = $("#symbols");
  const statusEl = $("#status");
  const saveBtn = $("#saveBtn");
  const entriesEl = $("#entries");
  const countEl = $("#count");
  const todayEl = $("#today");
  const drawer = $("#drawer");
  const settingsBtn = $("#settingsBtn");
  const themesEl = $("#themes");
  const motionSwitch = $("#motionSwitch");
  const aura = $("#aura");
  const readerEl = $("#reader");
  const readerScrim = $("#readerScrim");
  const readerTitle = $("#readerTitle");
  const readerLucid = $("#readerLucid");
  const readerTags = $("#readerTags");
  const readerText = $("#readerText");
  const readerFile = $("#readerFile");
  const closeReaderBtn = $("#closeReader");
  let readerOpener = null; // the entry that opened the reader, to return focus to

  const sidebarEl = $("#sidebar");
  const edgeEl = $("#edge");
  const tabDreams = $("#tabDreams");
  const journalEl = $("#journal");
  const closeSidebarBtn = $("#closeSidebar");
  const sideSearch = $("#sideSearch");
  const sideSort = $("#sideSort");
  const sideLucid = $("#sideLucid");
  const sideTags = $("#sideTags");
  const sideCount = $("#sideCount");
  const sideClear = $("#sideClear");
  const sideList = $("#sideList");

  /* ==========================================================================
     Small helpers
     ========================================================================== */
  function el(tag, props = {}, children = []) {
    const node = document.createElement(tag);
    Object.entries(props).forEach(([key, value]) => {
      if (key === "class") node.className = value;
      else if (key === "text") node.textContent = value;
      else if (key === "style") node.style.cssText = value;
      else node.setAttribute(key, value);
    });
    children.forEach((child) => node.append(child));
    return node;
  }

  function rnd(min, max) {
    return min + Math.random() * (max - min);
  }

  function norm(name) {
    return name.trim().toLowerCase();
  }

  function glyph(key, cls = "glyph") {
    const holder = document.createElement("span");
    holder.innerHTML =
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="' +
      SYMBOLS[key] +
      '"/></svg>';
    const svg = holder.firstElementChild;
    svg.setAttribute("class", cls);
    return svg;
  }

  // The visual mark of a tag: its symbol, or a plain coloured dot
  function tagMark(tag, cls) {
    return tag.symbol ? glyph(tag.symbol, cls) : el("span", { class: "dot" });
  }

  function lerpHex(a, b, t) {
    const pa = [1, 3, 5].map((i) => parseInt(a.substr(i, 2), 16));
    const pb = [1, 3, 5].map((i) => parseInt(b.substr(i, 2), 16));
    return "#" + pa.map((v, i) => Math.round(v + (pb[i] - v) * t).toString(16).padStart(2, "0")).join("");
  }

  function setStatus(message, kind) {
    statusEl.textContent = message;
    statusEl.className = "status" + (kind ? " " + kind : "");
  }

  /* ==========================================================================
     Dream date and lucidity
     ========================================================================== */
  function setNow() {
    const now = new Date();
    now.setMinutes(now.getMinutes() - now.getTimezoneOffset());
    dreamTime.value = now.toISOString().slice(0, 16);
  }

  function fileStamp(value) {
    // "2026-10-13T07:12" -> "2026-10-13_07-12"
    return value.replace("T", "_").replace(":", "-");
  }

  function prettyName(name) {
    const m = name.match(/^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})(?:_\d+)?\.md$/);
    if (!m) return name;
    const d = new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5]);
    return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  }

  function buildOrbs() {
    orbsEl.replaceChildren(
      ...LEVELS.map((label, i) => {
        const orb = el("button", {
          class: "orb",
          type: "button",
          role: "radio",
          "aria-checked": "false",
          "aria-label": i + ": " + label,
          title: label,
          style: "--i:" + i,
        });
        orb.append(el("i"));
        orb.addEventListener("click", () => setLevel(i));
        return orb;
      })
    );
  }

  function setLevel(value) {
    state.level = value;
    [...orbsEl.children].forEach((orb, i) => {
      orb.setAttribute("aria-checked", String(i === value));
      if (i === value) {
        orb.classList.remove("pulse");
        void orb.offsetWidth; // restart the animation
        orb.classList.add("pulse");
      }
    });
    captionEl.textContent = value + " · " + LEVELS[value];
  }

  /* ==========================================================================
     Tag library: the list of every tag, saved to tags.json
     ========================================================================== */
  function findTag(name) {
    return state.library.find((tag) => norm(tag.name) === norm(name)) || null;
  }

  function nextColor() {
    return COLORS[state.library.length % COLORS.length];
  }

  async function loadLibrary() {
    try {
      const parsed = JSON.parse(await invoke("load_tags"));
      state.library = Array.isArray(parsed) ? parsed : [];
    } catch (err) {
      state.library = [];
    }
  }

  async function persistLibrary() {
    try {
      await invoke("save_tags", { content: JSON.stringify(state.library, null, 2) });
    } catch (err) {
      setStatus("Could not save tags: " + err, "error");
    }
  }

  /* ==========================================================================
     Tags on the current dream
     ========================================================================== */
  function pillFor(name) {
    const tag = findTag(name) || { name, color: COLORS[0], symbol: null };
    const pill = el("button", { class: "pill", type: "button", title: "Edit " + name });
    pill.dataset.name = name;
    pill.style.setProperty("--tag", tag.color);
    pill.append(tagMark(tag), el("span", { text: name }));

    const remove = el("span", { class: "remove", role: "button", "aria-label": "Remove " + name, text: "×" });
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      removeTag(name);
    });
    pill.append(remove);

    pill.addEventListener("click", () => openDesigner(name));
    return pill;
  }

  function renderPills() {
    const existing = new Map([...pillsEl.children].map((node) => [node.dataset.name, node]));
    state.selected.forEach((name) => {
      let node = existing.get(name);
      if (!node) {
        node = pillFor(name);
      } else {
        const tag = findTag(name);
        if (tag) {
          node.style.setProperty("--tag", tag.color);
          const mark = node.querySelector(".glyph, .dot");
          if (mark) mark.replaceWith(tagMark(tag));
        }
      }
      pillsEl.appendChild(node);
      existing.delete(name);
    });
    existing.forEach((node) => {
      if (!node.classList.contains("leaving")) node.remove();
    });
  }

  function selectTag(raw) {
    const name = raw.replace(/,/g, "").trim().slice(0, 24);
    if (!name) return false;

    if (state.selected.some((n) => norm(n) === norm(name))) {
      // Already on this dream: gently highlight it instead of adding a duplicate
      const node = [...pillsEl.children].find((p) => norm(p.dataset.name) === norm(name));
      if (node) {
        node.classList.remove("pulse-tag");
        void node.offsetWidth;
        node.classList.add("pulse-tag");
      }
      return false;
    }

    let tag = findTag(name);
    if (!tag) {
      tag = { name, color: nextColor(), symbol: null };
      state.library.push(tag);
      persistLibrary();
    }
    state.selected.push(tag.name);
    renderPills();
    renderLibrary();
    return true;
  }

  function removeTag(name) {
    state.selected = state.selected.filter((n) => norm(n) !== norm(name));
    const node = [...pillsEl.children].find((p) => p.dataset.name === name);
    if (node) {
      node.classList.add("leaving");
      setTimeout(() => node.remove(), 230);
    }
    renderLibrary();
  }

  /* Library chips: shown while the tag field has focus */
  function renderLibrary() {
    const query = norm(tagInput.value);
    const available = state.library.filter(
      (tag) =>
        !state.selected.some((n) => norm(n) === norm(tag.name)) &&
        (!query || norm(tag.name).includes(query))
    );

    const chips = available.map((tag) => {
      const chip = el("button", { class: "chip", type: "button", title: "Add " + tag.name });
      chip.style.setProperty("--tag", tag.color);
      chip.append(tagMark(tag, "glyph"), el("span", { text: tag.name }));
      chip.addEventListener("click", () => {
        selectTag(tag.name);
        tagInput.focus();
      });
      return chip;
    });

    if (query && !findTag(query)) {
      chips.push(el("span", { class: "chip-empty", text: "Press Enter to create “" + tagInput.value.trim() + "”" }));
    }
    if (!chips.length) {
      chips.push(el("span", { class: "chip-empty", text: state.library.length ? "All your tags are on this dream" : "Your tags will gather here" }));
    }

    libraryEl.replaceChildren(...chips);
    libraryEl.classList.toggle("open", document.activeElement === tagInput);
  }

  tagInput.addEventListener("keydown", (event) => {
    if (event.ctrlKey) return; // Ctrl+Enter saves the dream instead
    if (event.key === "Enter" || event.key === ",") {
      event.preventDefault();
      selectTag(tagInput.value);
      tagInput.value = "";
      renderLibrary();
    } else if (event.key === "Backspace" && tagInput.value === "" && state.selected.length) {
      removeTag(state.selected[state.selected.length - 1]);
    } else if (event.key === "Escape") {
      tagInput.blur();
    }
  });

  tagInput.addEventListener("input", renderLibrary);
  tagInput.addEventListener("focus", () => {
    renderLibrary();
    libraryEl.classList.add("open");
  });
  tagInput.addEventListener("blur", () => {
    setTimeout(() => libraryEl.classList.remove("open"), 140);
  });

  // Keep focus in the field when a chip is pressed
  libraryEl.addEventListener("mousedown", (event) => event.preventDefault());

  // Clicking the empty part of the tag field focuses the input
  tagfieldEl.addEventListener("click", (event) => {
    if (event.target === tagfieldEl) tagInput.focus();
  });

  /* ==========================================================================
     Tag designer: name, colour, and symbol for a tag
     ========================================================================== */
  function buildSwatches() {
    swatchesEl.replaceChildren(
      ...COLORS.map((color) => {
        const swatch = el("button", {
          class: "swatch",
          type: "button",
          role: "radio",
          "aria-label": "Colour " + color,
          "aria-checked": String(state.designer.color === color),
          style: "--c:" + color,
        });
        swatch.addEventListener("click", () => {
          state.designer.color = color;
          syncDesigner();
        });
        return swatch;
      })
    );
  }

  function buildSymbols() {
    const keys = [null, ...Object.keys(SYMBOLS)];
    symbolsEl.replaceChildren(
      ...keys.map((key) => {
        const button = el("button", {
          class: "sym",
          type: "button",
          role: "radio",
          "aria-label": key ? key : "No symbol, just a dot",
          "aria-checked": String(state.designer.symbol === key),
        });
        button.append(key ? glyph(key, "") : el("span", { class: "dot" }));
        button.addEventListener("click", () => {
          state.designer.symbol = key;
          syncDesigner();
        });
        return button;
      })
    );
  }

  function syncDesigner() {
    const d = state.designer;
    if (!d) return;
    designerEl.style.setProperty("--tag", d.color);
    designerPreview.style.setProperty("--tag", d.color);
    designerPreview.replaceChildren(
      tagMark({ color: d.color, symbol: d.symbol }),
      el("span", { text: designerName.value.trim() || "Your tag" })
    );
    [...swatchesEl.children].forEach((swatch, i) => {
      swatch.setAttribute("aria-checked", String(COLORS[i] === d.color));
    });
    [...symbolsEl.children].forEach((button, i) => {
      const key = i === 0 ? null : Object.keys(SYMBOLS)[i - 1];
      button.setAttribute("aria-checked", String(key === d.symbol));
    });
  }

  function openDesigner(name) {
    const existing = name ? findTag(name) : null;
    state.designer = {
      editing: existing ? existing.name : null,
      color: existing ? existing.color : nextColor(),
      symbol: existing ? existing.symbol : null,
    };
    designerTitle.textContent = existing ? "Edit tag" : "New tag";
    designerSave.textContent = existing ? "Save changes" : "Create tag";
    designerName.value = existing ? existing.name : name || "";
    designerError.textContent = "";
    buildSwatches();
    buildSymbols();
    syncDesigner();
    designerEl.classList.add("open");
    setTimeout(() => {
      designerName.focus();
      designerName.select();
    }, 40);
  }

  function closeDesigner() {
    state.designer = null;
    designerEl.classList.remove("open");
  }

  async function saveDesigner() {
    const d = state.designer;
    if (!d) return;

    const name = designerName.value.replace(/,/g, "").trim().slice(0, 24);
    if (!name) {
      designerError.textContent = "Give your tag a name first.";
      designerName.focus();
      return;
    }

    const clash = state.library.find((tag) => norm(tag.name) === norm(name) && tag.name !== d.editing);
    if (d.editing && clash) {
      designerError.textContent = "You already have a tag called that.";
      designerName.focus();
      return;
    }

    let target = d.editing ? findTag(d.editing) : findTag(name);
    if (target) {
      const oldName = target.name;
      target.name = name;
      target.color = d.color;
      target.symbol = d.symbol;
      if (oldName !== name) {
        state.selected = state.selected.map((n) => (norm(n) === norm(oldName) ? name : n));
      }
    } else {
      target = { name, color: d.color, symbol: d.symbol };
      state.library.push(target);
    }

    if (!state.selected.some((n) => norm(n) === norm(name))) state.selected.push(name);

    await persistLibrary();
    renderPills();
    renderLibrary();
    closeDesigner();
    tagInput.focus();
    burst(designerSave, 10);
  }

  designBtn.addEventListener("click", () => {
    const typed = tagInput.value.trim();
    tagInput.value = "";
    openDesigner(typed);
  });

  designerName.addEventListener("input", syncDesigner);
  designerName.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      saveDesigner();
    } else if (event.key === "Escape") {
      closeDesigner();
      tagInput.focus();
    }
  });
  $("#designerSave").addEventListener("click", saveDesigner);
  $("#designerCancel").addEventListener("click", () => {
    closeDesigner();
    tagInput.focus();
  });

  // Click anywhere outside the designer to close it
  document.addEventListener("pointerdown", (event) => {
    if (!state.designer) return;
    if (event.target.closest("#designer, #designBtn, .pill")) return;
    closeDesigner();
  });

  /* ==========================================================================
     Saving a dream
     ========================================================================== */
  async function saveDream() {
    if (tagInput.value.trim()) {
      selectTag(tagInput.value);
      tagInput.value = "";
    }

    const text = dreamEl.value.trim();
    if (!text) {
      setStatus("Nothing to save yet.", "error");
      dreamEl.focus();
      return;
    }

    const header = [
      "---",
      "date: " + dreamTime.value.replace("T", " "),
      "lucidity: " + state.level,
      "tags: [" + state.selected.join(", ") + "]",
      "---",
      "",
    ].join("\n");

    const filename = fileStamp(dreamTime.value) + ".md";

    try {
      const saved = await invoke("save_dream", { filename, content: header + text + "\n" });
      setStatus("Saved " + prettyName(saved), "ok");
      burst(saveBtn, 16);
      dreamEl.value = "";
      state.selected = [];
      renderPills();
      renderLibrary();
      setLevel(0);
      setNow();
      refreshRecent();
    } catch (err) {
      setStatus("Could not save: " + err, "error");
    }
  }

  saveBtn.addEventListener("click", saveDream);

  document.addEventListener("keydown", (event) => {
    if (event.ctrlKey && event.key === "Enter") {
      event.preventDefault();
      saveDream();
    } else if (event.key === "Escape") {
      if (state.designer) {
        closeDesigner();
        tagInput.focus();
      } else if (readerEl.classList.contains("open")) {
        closeReader();
      } else if (drawer.classList.contains("open")) {
        closeSettings();
      } else if (sidebarEl.classList.contains("open")) {
        hideSidebar();
      }
    }
  });

  /* ==========================================================================
     Recent dreams
     ========================================================================== */
  async function refreshRecent() {
    try {
      const pairs = await invoke("read_dreams");
      state.journal = pairs.map(([name, content]) => ({ name, ...parseDream(content) }));
      state.dreams = state.journal.map((dream) => dream.name);
      renderRecent();
      renderSidebar();
    } catch (err) {
      entriesEl.replaceChildren(el("div", { class: "empty", text: "Could not load entries." }));
      sideList.replaceChildren(el("div", { class: "empty", text: "Could not load dreams." }));
    }
  }

  function renderRecent() {
    const names = state.dreams;
    countEl.textContent = names.length ? names.length + " saved" : "";
    if (names.length === 0) {
      entriesEl.replaceChildren(el("div", { class: "empty", text: "Your first dream will show up here." }));
      return;
    }

    const shown = state.showAll ? names : names.slice(0, RECENT_LIMIT);
    const rows = shown.map((name, i) => {
      const row = el("button", { class: "entry", type: "button", title: "Open this dream", style: "--n:" + Math.min(i, RECENT_LIMIT) });
      row.append(el("span", { class: "entry-title", text: prettyName(name) }));
      row.append(el("span", { class: "entry-file", text: name }));
      row.addEventListener("click", () => openDream(name, row));
      return row;
    });

    if (names.length > RECENT_LIMIT) {
      const more = el("button", {
        class: "more",
        type: "button",
        text: state.showAll ? "Show fewer" : "Show all " + names.length,
      });
      more.addEventListener("click", () => {
        state.showAll = !state.showAll;
        renderRecent();
      });
      rows.push(more);
    }
    entriesEl.replaceChildren(...rows);
  }

  /* ==========================================================================
     Reader: open a saved dream file and show it
     ========================================================================== */
  // Split a saved file into its header (date, lucidity, tags) and its text
  function parseDream(text) {
    const dream = { lucidity: 0, tags: [], body: text };
    const match = text.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?/);
    if (!match) return dream;

    match[1].split(/\r?\n/).forEach((line) => {
      const colon = line.indexOf(":");
      if (colon < 0) return;
      const key = line.slice(0, colon).trim();
      const value = line.slice(colon + 1).trim();
      if (key === "lucidity") {
        dream.lucidity = Math.min(LEVELS.length - 1, Math.max(0, parseInt(value, 10) || 0));
      } else if (key === "tags") {
        const inner = value.replace(/^\[/, "").replace(/\]$/, "").trim();
        dream.tags = inner ? inner.split(",").map((t) => t.trim()).filter(Boolean) : [];
      }
    });

    dream.body = text.slice(match[0].length).replace(/\r\n/g, "\n").trim();
    return dream;
  }

  // A read-only tag pill, coloured from the tag library
  function tagChip(name) {
    const tag = findTag(name) || { name, color: COLORS[0], symbol: null };
    const chip = el("span", { class: "pill static" });
    chip.style.setProperty("--tag", tag.color);
    chip.append(tagMark(tag), el("span", { text: name }));
    return chip;
  }

  async function openDream(name, opener) {
    readerOpener = opener || document.activeElement;
    readerTitle.textContent = prettyName(name);
    readerFile.textContent = name;

    try {
      const dream = parseDream(await invoke("read_dream", { filename: name }));
      readerLucid.textContent = dream.lucidity + " · " + LEVELS[dream.lucidity];
      readerTags.replaceChildren(...(dream.tags.length ? dream.tags.map(tagChip) : [el("span", { class: "chip-empty", text: "No tags" })]));
      readerText.textContent = dream.body || "This dream was saved without words.";
      readerText.classList.toggle("empty-text", !dream.body);
    } catch (err) {
      readerLucid.textContent = "";
      readerTags.replaceChildren();
      readerText.textContent = "Could not open this dream: " + err;
      readerText.classList.remove("empty-text");
    }

    readerEl.classList.add("open");
    readerEl.setAttribute("aria-hidden", "false");
    setTimeout(() => closeReaderBtn.focus(), 50);
  }

  function closeReader() {
    readerEl.classList.remove("open");
    readerEl.setAttribute("aria-hidden", "true");
    if (readerOpener && readerOpener.isConnected) readerOpener.focus();
    readerOpener = null;
    if (!sideHover) scheduleSidebarHide(); // the pointer may have left while the reader was up
  }

  closeReaderBtn.addEventListener("click", closeReader);
  readerScrim.addEventListener("click", closeReader);

  /* ==========================================================================
     Left sidebar: slides out when the pointer reaches the left edge.
     The rail holds the tabs; the Dreams tab opens a detailed journal beside it.
     ========================================================================== */
  let sideHover = false;   // the pointer is over the sidebar
  let sideHideTimer = 0;

  function showSidebar() {
    clearTimeout(sideHideTimer);
    if (sidebarEl.classList.contains("open")) return;
    if (drawer.classList.contains("open") || readerEl.classList.contains("open")) return;
    sidebarEl.classList.add("open");
    sidebarEl.setAttribute("aria-hidden", "false");
  }

  function hideSidebar() {
    clearTimeout(sideHideTimer);
    sideHover = false;
    sidebarEl.classList.remove("open");
    sidebarEl.setAttribute("aria-hidden", "true");
  }

  // Close shortly after the pointer leaves, unless the pointer came back,
  // the user is typing in the sidebar, or a dream is open in the reader
  function scheduleSidebarHide() {
    clearTimeout(sideHideTimer);
    sideHideTimer = setTimeout(() => {
      const typing = sidebarEl.contains(document.activeElement) && document.activeElement.matches("input, select");
      if (sideHover || typing || readerEl.classList.contains("open")) return;
      hideSidebar();
    }, 320);
  }

  function setDreamsOpen(open) {
    state.dreamsOpen = open;
    sidebarEl.classList.toggle("expanded", open);
    tabDreams.classList.toggle("active", open);
    tabDreams.setAttribute("aria-expanded", String(open));
    journalEl.inert = !open;
    if (open) renderSidebar();
  }

  // A dream's date spelled out in full, so "october" or "monday" can be searched
  function dateWords(name) {
    const m = name.match(/^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})(?:_\d+)?\.md$/);
    if (!m) return name;
    const d = new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5]);
    return d.toLocaleString(undefined, { dateStyle: "full", timeStyle: "short" });
  }

  // The dreams that pass the sidebar's search, lucidity and tag filters, in its sort order
  function matchingDreams() {
    const f = state.filters;
    const query = norm(f.query);
    const list = state.journal.filter((dream) => {
      if (f.lucidity !== "any" && dream.lucidity !== Number(f.lucidity)) return false;
      if (!f.tags.every((wanted) => dream.tags.some((t) => norm(t) === norm(wanted)))) return false;
      if (query) {
        const words = norm([prettyName(dream.name), dateWords(dream.name), dream.name, dream.tags.join(" "), dream.body].join(" "));
        if (!words.includes(query)) return false;
      }
      return true;
    });

    // The journal is already newest first; stable sorts keep that order within ties
    if (f.sort === "oldest") list.reverse();
    else if (f.sort === "lucid") list.sort((a, b) => b.lucidity - a.lucidity);
    else if (f.sort === "calm") list.sort((a, b) => a.lucidity - b.lucidity);
    return list;
  }

  function toggleSideTag(name) {
    const f = state.filters;
    const on = f.tags.some((t) => norm(t) === norm(name));
    f.tags = on ? f.tags.filter((t) => norm(t) !== norm(name)) : [...f.tags, name];
    renderSidebar();
  }

  function sideItem(dream, i) {
    const item = el("button", { class: "side-item", type: "button", title: "Open this dream", style: "--n:" + Math.min(i, 12) });
    item.append(
      el("span", { class: "side-head" }, [
        el("span", { class: "side-date", text: prettyName(dream.name) }),
        el("span", { class: "side-level", text: dream.lucidity + " · " + LEVELS[dream.lucidity] }),
      ])
    );
    if (dream.tags.length) item.append(el("span", { class: "side-tags" }, dream.tags.map(tagChip)));
    const snippet = dream.body ? dream.body.replace(/\s+/g, " ") : "No words saved";
    item.append(el("span", { class: "side-snippet", text: snippet }));
    item.addEventListener("click", () => openDream(dream.name, item));
    return item;
  }

  function renderSidebar() {
    const f = state.filters;
    const total = state.journal.length;

    // Lucidity: Any, or one exact level
    const levels = [["any", "Any"], ...LEVELS.map((_, i) => [String(i), String(i)])];
    sideLucid.replaceChildren(
      ...levels.map(([value, label]) => {
        const seg = el("button", {
          class: "seg",
          type: "button",
          role: "radio",
          "aria-checked": String(f.lucidity === value),
          title: value === "any" ? "Any level" : value + " · " + LEVELS[Number(value)],
          text: label,
        });
        seg.addEventListener("click", () => {
          f.lucidity = value;
          renderSidebar();
        });
        return seg;
      })
    );

    // Tags: every tag used on a saved dream, pick any number (all must match)
    const used = new Map();
    state.journal.forEach((dream) => dream.tags.forEach((t) => used.has(norm(t)) || used.set(norm(t), t)));
    const tagNames = [...used.values()].sort((a, b) => a.localeCompare(b));
    sideTags.replaceChildren(
      ...(tagNames.length
        ? tagNames.map((name) => {
            const tag = findTag(name) || { name, color: COLORS[0], symbol: null };
            const on = f.tags.some((t) => norm(t) === norm(name));
            const chip = el("button", { class: "chip", type: "button", "aria-pressed": String(on), title: "Filter by " + name });
            chip.style.setProperty("--tag", tag.color);
            chip.append(tagMark(tag, "glyph"), el("span", { text: name }));
            chip.addEventListener("click", () => toggleSideTag(name));
            return chip;
          })
        : [el("span", { class: "chip-empty", text: "Tags you add to dreams will appear here" })])
    );

    // The list itself
    const list = matchingDreams();
    const filtered = Boolean(f.query) || f.tags.length > 0 || f.lucidity !== "any";
    sideClear.hidden = !filtered;
    sideCount.textContent = !total ? "" : filtered ? list.length + " of " + total : total + (total === 1 ? " dream" : " dreams");

    if (!total) sideList.replaceChildren(el("div", { class: "empty", text: "Your saved dreams will appear here." }));
    else if (!list.length) sideList.replaceChildren(el("div", { class: "empty", text: "No dreams match these filters." }));
    else sideList.replaceChildren(...list.map(sideItem));
  }

  tabDreams.addEventListener("click", () => setDreamsOpen(!state.dreamsOpen));
  closeSidebarBtn.addEventListener("click", hideSidebar);

  sideSearch.addEventListener("input", () => {
    state.filters.query = sideSearch.value;
    renderSidebar();
  });
  sideSort.addEventListener("change", () => {
    state.filters.sort = sideSort.value;
    renderSidebar();
  });
  sideClear.addEventListener("click", () => {
    Object.assign(state.filters, { query: "", lucidity: "any", tags: [] });
    sideSearch.value = "";
    renderSidebar();
  });

  // Hover behaviour: the edge opens it, leaving it closes it
  edgeEl.addEventListener("pointerenter", showSidebar);
  sidebarEl.addEventListener("pointerenter", () => {
    sideHover = true;
    clearTimeout(sideHideTimer);
  });
  sidebarEl.addEventListener("pointerleave", () => {
    sideHover = false;
    scheduleSidebarHide();
  });
  // Tabbing out of the sidebar (with the pointer away) also closes it
  sidebarEl.addEventListener("focusout", () => {
    if (!sideHover) scheduleSidebarHide();
  });

  /* ==========================================================================
     Feedback effects: button ripple and the star burst on save
     ========================================================================== */
  document.addEventListener("pointerdown", (event) => {
    const button = event.target.closest(".btn");
    if (!button) return;
    const rect = button.getBoundingClientRect();
    const ripple = el("span", { class: "ripple" });
    ripple.style.left = event.clientX - rect.left + "px";
    ripple.style.top = event.clientY - rect.top + "px";
    button.append(ripple);
    setTimeout(() => ripple.remove(), 750);
  });

  function burst(anchor, count) {
    if (!state.motion) return;
    const rect = anchor.getBoundingClientRect();
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    for (let i = 0; i < count; i++) {
      const angle = rnd(0, Math.PI * 2);
      const distance = rnd(40, 120);
      const spark = el("span", { class: "spark" });
      spark.style.left = cx + "px";
      spark.style.top = cy + "px";
      spark.style.setProperty("--x", Math.cos(angle) * distance + "px");
      spark.style.setProperty("--y", Math.sin(angle) * distance + "px");
      document.body.append(spark);
      setTimeout(() => spark.remove(), 950);
    }
  }

  /* ==========================================================================
     Worlds: the sky canvas for each theme
     ========================================================================== */
  const canvas = $("#sky");
  const ctx = canvas.getContext("2d");
  let width = 0;
  let height = 0;
  let stars = [];
  let motes = [];
  let orbsField = [];
  let frameId = 0;
  let lastFrame = 0;

  function seedSky() {
    const count = Math.min(240, Math.round((width * height) / 6500));
    stars = Array.from({ length: count }, () => ({
      x: rnd(0, width),
      y: rnd(0, height),
      r: Math.random() < 0.08 ? rnd(1.2, 1.9) : rnd(0.4, 1.1),
      a: rnd(0.35, 0.95),
      s: rnd(0.6, 2.2),
      o: rnd(0, 6.28),
      vy: rnd(-0.06, -0.012),
    }));
    motes = Array.from({ length: 40 }, () => ({
      x: rnd(0, width),
      y: rnd(0, height),
      bx: 0,
      r: rnd(1, 3.2),
      a: rnd(0.15, 0.5),
      vy: rnd(-0.22, -0.06),
      sway: rnd(6, 22),
      f: rnd(0.2, 0.6),
      o: rnd(0, 6.28),
    }));
    motes.forEach((m) => (m.bx = m.x));
    orbsField = Array.from({ length: 4 }, () => ({
      x: rnd(0, width),
      y: rnd(0, height),
      r: rnd(140, 260),
      o: rnd(0, 6.28),
      sp: rnd(0.03, 0.08),
    }));
  }

  function resizeSky() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    width = window.innerWidth;
    height = window.innerHeight;
    canvas.width = Math.floor(width * dpr);
    canvas.height = Math.floor(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    seedSky();
  }

  function drawNight(t, dt) {
    ctx.clearRect(0, 0, width, height);
    stars.forEach((s) => {
      s.y += s.vy * dt * 60;
      if (s.y < -3) {
        s.y = height + 3;
        s.x = rnd(0, width);
      }
      const twinkle = 0.55 + 0.45 * Math.sin(t * s.s + s.o);
      ctx.fillStyle = "#e8e6ff";
      ctx.globalAlpha = s.a * twinkle;
      ctx.beginPath();
      ctx.arc(s.x, s.y, s.r, 0, Math.PI * 2);
      ctx.fill();
      if (s.r > 1.5) {
        ctx.globalAlpha = s.a * twinkle * 0.12;
        ctx.beginPath();
        ctx.arc(s.x, s.y, s.r * 4, 0, Math.PI * 2);
        ctx.fill();
      }
    });
    ctx.globalAlpha = 1;
  }

  function drawDawn(t, dt) {
    ctx.clearRect(0, 0, width, height);
    const horizon = ctx.createLinearGradient(0, height * 0.55, 0, height);
    horizon.addColorStop(0, "rgba(255, 170, 130, 0)");
    horizon.addColorStop(1, "rgba(255, 170, 130, 0.16)");
    ctx.fillStyle = horizon;
    ctx.fillRect(0, height * 0.55, width, height * 0.45);

    motes.forEach((m) => {
      m.y += m.vy * dt * 60;
      if (m.y < -10) {
        m.y = height + 10;
        m.bx = rnd(0, width);
      }
      const x = m.bx + Math.sin(t * m.f + m.o) * m.sway;
      ctx.fillStyle = "rgba(255, 196, 160, 1)";
      ctx.globalAlpha = m.a;
      ctx.beginPath();
      ctx.arc(x, m.y, m.r, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalAlpha = m.a * 0.2;
      ctx.beginPath();
      ctx.arc(x, m.y, m.r * 4, 0, Math.PI * 2);
      ctx.fill();
    });
    ctx.globalAlpha = 1;
  }

  function drawLucid(t, dt) {
    ctx.clearRect(0, 0, width, height);
    const grid = 44;
    ctx.strokeStyle = "rgba(94, 242, 214, 0.065)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let x = 0; x <= width; x += grid) {
      ctx.moveTo(Math.floor(x) + 0.5, 0);
      ctx.lineTo(Math.floor(x) + 0.5, height);
    }
    for (let y = 0; y <= height; y += grid) {
      ctx.moveTo(0, Math.floor(y) + 0.5);
      ctx.lineTo(width, Math.floor(y) + 0.5);
    }
    ctx.stroke();

    orbsField.forEach((o) => {
      const cx = o.x + Math.cos(t * o.sp + o.o) * 70;
      const cy = o.y + Math.sin(t * o.sp * 0.8 + o.o) * 50;
      const glow = ctx.createRadialGradient(cx, cy, 0, cx, cy, o.r);
      glow.addColorStop(0, "rgba(111, 183, 255, 0.08)");
      glow.addColorStop(1, "rgba(111, 183, 255, 0)");
      ctx.fillStyle = glow;
      ctx.fillRect(cx - o.r, cy - o.r, o.r * 2, o.r * 2);
    });

    const scanY = ((t * 70) % (height + 180)) - 90;
    const band = ctx.createLinearGradient(0, scanY - 70, 0, scanY + 70);
    band.addColorStop(0, "rgba(94, 242, 214, 0)");
    band.addColorStop(0.5, "rgba(94, 242, 214, 0.1)");
    band.addColorStop(1, "rgba(94, 242, 214, 0)");
    ctx.fillStyle = band;
    ctx.fillRect(0, scanY - 70, width, 140);
    ctx.fillStyle = "rgba(94, 242, 214, 0.28)";
    ctx.fillRect(0, Math.round(scanY), width, 1);
  }

  function drawSky(t, dt) {
    if (state.theme === "dawn") drawDawn(t, dt);
    else if (state.theme === "lucid") drawLucid(t, dt);
    else drawNight(t, dt);
  }

  function frame(now) {
    const t = now / 1000;
    const dt = Math.min(0.05, (now - lastFrame) / 1000);
    lastFrame = now;
    drawSky(t, dt);
    if (state.motion && !document.hidden) frameId = requestAnimationFrame(frame);
  }

  function startSky() {
    cancelAnimationFrame(frameId);
    if (state.motion) {
      lastFrame = performance.now();
      frameId = requestAnimationFrame(frame);
    } else {
      drawSky(0, 0); // one still frame
    }
  }

  // Dawn follows the real time of day
  function updateDawnSky() {
    if (state.theme !== "dawn") {
      root.style.removeProperty("--sky-top");
      root.style.removeProperty("--sky-bottom");
      return;
    }
    const now = new Date();
    const hour = now.getHours() + now.getMinutes() / 60;
    let i = 0;
    while (i < SKY_STOPS.length - 2 && hour > SKY_STOPS[i + 1][0]) i++;
    const [h0, top0, bottom0] = SKY_STOPS[i];
    const [h1, top1, bottom1] = SKY_STOPS[i + 1];
    const t = Math.min(1, Math.max(0, (hour - h0) / (h1 - h0)));
    root.style.setProperty("--sky-top", lerpHex(top0, top1, t));
    root.style.setProperty("--sky-bottom", lerpHex(bottom0, bottom1, t));
  }

  /* Soft light that follows the cursor */
  let auraX = -999;
  let auraY = -999;
  let targetX = 0;
  let targetY = 0;
  let auraRunning = false;

  function runAura() {
    auraRunning = true;
    const step = () => {
      auraX += (targetX - auraX) * 0.12;
      auraY += (targetY - auraY) * 0.12;
      aura.style.transform = "translate3d(" + auraX + "px, " + auraY + "px, 0)";
      if (Math.abs(targetX - auraX) + Math.abs(targetY - auraY) > 0.3 && state.motion) {
        requestAnimationFrame(step);
      } else {
        auraRunning = false;
      }
    };
    requestAnimationFrame(step);
  }

  window.addEventListener(
    "pointermove",
    (event) => {
      if (!state.motion) return;
      targetX = event.clientX;
      targetY = event.clientY;
      if (!auraRunning) runAura();
    },
    { passive: true }
  );

  /* ==========================================================================
     Settings: visual world and ambient motion
     ========================================================================== */
  function buildThemes() {
    themesEl.replaceChildren(
      ...THEMES.map((theme) => {
        const option = el("button", {
          class: "theme-opt",
          type: "button",
          role: "radio",
          "aria-checked": String(state.theme === theme.id),
        });
        option.dataset.themeId = theme.id;
        option.append(
          el("div", { class: "preview " + theme.preview }),
          el("div", {}, [el("strong", { text: theme.name }), el("span", { class: "desc", text: theme.desc })])
        );
        option.addEventListener("click", () => setTheme(theme.id));
        return option;
      })
    );
  }

  function setTheme(id) {
    state.theme = id;
    root.dataset.theme = id;
    prefs.set("theme", id);
    [...themesEl.children].forEach((option) => {
      option.setAttribute("aria-checked", String(option.dataset.themeId === id));
    });
    updateDawnSky();
    resizeSky();
    startSky();
  }

  function setMotion(on) {
    state.motion = on;
    prefs.set("motion", on);
    motionSwitch.setAttribute("aria-checked", String(on));
    body.classList.toggle("still", !on);
    body.classList.toggle("motion-on", on);
    startSky();
    if (on) runAura();
  }

  function openSettings() {
    drawer.classList.add("open");
    drawer.setAttribute("aria-hidden", "false");
    setTimeout(() => $("#closeSettings").focus(), 50);
  }

  function closeSettings() {
    drawer.classList.remove("open");
    drawer.setAttribute("aria-hidden", "true");
    settingsBtn.focus();
  }

  settingsBtn.addEventListener("click", openSettings);
  $("#closeSettings").addEventListener("click", closeSettings);
  $("#scrim").addEventListener("click", closeSettings);
  motionSwitch.addEventListener("click", () => setMotion(!state.motion));

  document.addEventListener("visibilitychange", () => {
    if (!document.hidden && state.motion) startSky();
  });

  window.addEventListener("resize", () => {
    resizeSky();
    startSky();
  });

  setInterval(updateDawnSky, 60 * 1000);

  /* ==========================================================================
     Start
     ========================================================================== */
  async function init() {
    const savedTheme = prefs.get("theme", "night");
    state.theme = THEMES.some((t) => t.id === savedTheme) ? savedTheme : "night";
    root.dataset.theme = state.theme;

    const reduced = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    state.motion = prefs.get("motion", !reduced);
    body.classList.toggle("still", !state.motion);
    body.classList.toggle("motion-on", state.motion);
    motionSwitch.setAttribute("aria-checked", String(state.motion));

    todayEl.textContent = new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });

    buildThemes();
    buildOrbs();
    setLevel(0);
    setNow();
    renderPills();

    await loadLibrary();
    renderLibrary();
    refreshRecent();

    updateDawnSky();
    resizeSky();
    startSky();
    if (state.motion) runAura();
  }

  init();
})();

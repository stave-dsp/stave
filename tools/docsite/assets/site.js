// The site's behaviour: theme, search, copy buttons, the outline, the
// crate graph. Runs on the site's pages and (injected) on rustdoc's.
(function () {
  "use strict";
  var root = window.STAVE_ROOT != null ? window.STAVE_ROOT : (document.body.getAttribute("data-root") || "");

  // ---------------------------------------------------------------- theme
  function current() {
    var t = document.documentElement.dataset.theme;
    if (t) return t;
    return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  function setTheme(t) {
    document.documentElement.dataset.theme = t;
    try {
      localStorage.setItem("stave-theme", t);
      // Rustdoc's own settings, so the API pages follow.
      localStorage.setItem("rustdoc-use-system-theme", "false");
      localStorage.setItem("rustdoc-theme", t);
    } catch (e) {}
  }
  document.querySelectorAll(".theme-toggle").forEach(function (b) {
    b.addEventListener("click", function () { setTheme(current() === "dark" ? "light" : "dark"); });
  });

  // --------------------------------------------------------------- search
  var dialog, input, list, sel = 0, hits = [];
  var KIND = { tutorial: "tutorial", concept: "concept", "how-to": "how-to", guide: "guide", reference: "reference" };
  function build() {
    dialog = document.createElement("div");
    dialog.className = "search-dialog";
    dialog.innerHTML =
      '<div class="search-panel" role="dialog" aria-label="Search the docs">' +
      '<input type="search" placeholder="Search guides and API — Plugin, smoothing, FaceDriver…" aria-label="Search" autocomplete="off" spellcheck="false">' +
      '<ul class="search-results" role="listbox"></ul>' +
      '<div class="search-foot"><span><kbd>↑</kbd> <kbd>↓</kbd> to move</span><span><kbd>↵</kbd> to open</span><span><kbd>esc</kbd> to close</span></div></div>';
    document.body.appendChild(dialog);
    input = dialog.querySelector("input");
    list = dialog.querySelector(".search-results");
    dialog.addEventListener("click", function (e) { if (e.target === dialog) close(); });
    input.addEventListener("input", function () { run(input.value); });
    input.addEventListener("keydown", function (e) {
      if (e.key === "ArrowDown") { sel = Math.min(sel + 1, hits.length - 1); paint(); e.preventDefault(); }
      else if (e.key === "ArrowUp") { sel = Math.max(sel - 1, 0); paint(); e.preventDefault(); }
      else if (e.key === "Enter" && hits[sel]) { location.href = root + hits[sel][1]; close(); }
      else if (e.key === "Escape") { close(); }
    });
  }
  var loading = false;
  function open(q) {
    if (!dialog) build();
    // The index is large; it loads on the first search.
    if (!window.STAVE_INDEX && !loading) {
      loading = true;
      var s = document.createElement("script");
      s.src = root + "search-index.js";
      s.onload = function () { run(input.value); };
      document.head.appendChild(s);
    }
    dialog.classList.add("open");
    input.value = q || input.value || "";
    run(input.value);
    setTimeout(function () { input.focus(); input.select(); }, 0);
  }
  function close() { if (dialog) dialog.classList.remove("open"); }

  function score(e, q, words) {
    var t = e[0].toLowerCase();
    var last = t.split("::").pop();
    var s = 0;
    if (last === q || t === q) s += 100;
    else if (last.indexOf(q) === 0) s += 60;
    else if (t.indexOf(q) >= 0) s += 35;
    var hay = (t + " " + e[3] + " " + e[4]).toLowerCase();
    for (var i = 0; i < words.length; i++) {
      if (hay.indexOf(words[i]) < 0) return 0;
      s += 4;
    }
    if (KIND[e[2]]) s += 12; // prose first, on a tie
    if (e[2] === "method" || e[2] === "field" || e[2] === "variant" || e[2] === "type" || e[2] === "const") s -= 6;
    s -= Math.min(t.length, 60) / 20;
    return s;
  }
  function run(q) {
    q = q.trim().toLowerCase();
    var idx = window.STAVE_INDEX || [];
    hits = [];
    if (q) {
      var words = q.split(/\s+/);
      var scored = [];
      for (var i = 0; i < idx.length; i++) {
        var s = score(idx[i], q, words);
        if (s > 0) scored.push([s, idx[i]]);
      }
      scored.sort(function (a, b) { return b[0] - a[0]; });
      hits = scored.slice(0, 40).map(function (x) { return x[1]; });
    }
    sel = 0;
    paint(q);
  }
  function esc(s) { return s.replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; }); }
  function paint(q) {
    if (!list) return;
    if (!hits.length) {
      if (!window.STAVE_INDEX && input.value.trim()) { list.innerHTML = '<li class="empty">Loading the index…</li>'; return; }
      list.innerHTML = input.value.trim() ? '<li class="empty">Nothing matches “' + esc(input.value.trim()) + '”.</li>' : '<li class="empty">Type to search every guide, concept and API item.</li>';
      return;
    }
    list.innerHTML = hits.map(function (e, i) {
      var api = !KIND[e[2]];
      var ctx = e[3] ? e[3] + (e[4] ? " — " : "") : "";
      return '<li><a href="' + root + e[1] + '"' + (i === sel ? ' class="sel"' : "") + '><span class="k' + (api ? " api" : "") + '">' + esc(e[2]) + '</span><span class="t' + (api ? " mono" : "") + '">' + esc(e[0]) + '</span><span class="d">' + esc(ctx + e[4]) + "</span></a></li>";
    }).join("");
    var s = list.querySelector(".sel");
    if (s && s.scrollIntoView) s.scrollIntoView({ block: "nearest" });
  }
  document.querySelectorAll(".search-open").forEach(function (b) { b.addEventListener("click", function () { open(); }); });
  document.addEventListener("keydown", function (e) {
    var t = e.target;
    var typing = t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
    if (!typing && (e.key === "/" || (e.key === "k" && (e.metaKey || e.ctrlKey)))) {
      // On rustdoc's pages "/" and "s" belong to its own search; ⌘K is ours.
      if (window.STAVE_ROOT != null && e.key === "/") return;
      e.preventDefault();
      open();
    }
  });
  var m = /[?&]search=([^&]*)/.exec(location.search);
  if (m) open(decodeURIComponent(m[1].replace(/\+/g, " ")));

  // -------------------------------------------------------- code blocks
  document.querySelectorAll(".doc pre").forEach(function (pre) {
    var b = document.createElement("button");
    b.className = "copy";
    b.type = "button";
    b.textContent = "Copy";
    b.addEventListener("click", function () {
      var code = pre.querySelector("code") || pre;
      navigator.clipboard.writeText(code.innerText).then(function () {
        b.textContent = "Copied";
        setTimeout(function () { b.textContent = "Copy"; }, 1200);
      });
    });
    pre.appendChild(b);
  });

  // -------------------------------------------------------------- sidebar
  document.querySelectorAll(".sidebar-toggle").forEach(function (b) {
    b.addEventListener("click", function () {
      var nav = b.closest(".sidebar");
      var openNow = nav.classList.toggle("open");
      b.setAttribute("aria-expanded", openNow ? "true" : "false");
    });
  });

  // -------------------------------------------------------------- outline
  var links = Array.prototype.slice.call(document.querySelectorAll(".outline a"));
  if (links.length && "IntersectionObserver" in window) {
    var byId = {};
    links.forEach(function (a) { byId[decodeURIComponent(a.hash.slice(1))] = a; });
    var obs = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (en.isIntersecting && byId[en.target.id]) {
          links.forEach(function (a) { a.classList.remove("active"); });
          byId[en.target.id].classList.add("active");
        }
      });
    }, { rootMargin: "-70px 0px -70% 0px" });
    Object.keys(byId).forEach(function (id) { var h = document.getElementById(id); if (h) obs.observe(h); });
  }

  // ---------------------------------------------------------- crate graph
  document.querySelectorAll(".graph .node").forEach(function (n) {
    var name = n.getAttribute("data-name");
    function hot(on) {
      n.classList.toggle("hot", on);
      document.querySelectorAll('.graph .edge[data-from="' + name + '"], .graph .edge[data-to="' + name + '"]').forEach(function (e) { e.classList.toggle("hot", on); });
    }
    n.addEventListener("mouseenter", function () { hot(true); });
    n.addEventListener("mouseleave", function () { hot(false); });
  });
})();

/* ==========================================================================
   Zero Layer — site behaviour
   Theme, language, navigation, tabs, copy buttons, terminal demo, reveals.

   Motion rules: only transform and opacity are animated, scroll work is
   batched into one requestAnimationFrame, and layout is read once and cached,
   so nothing forces a synchronous reflow while the page is moving.
   ========================================================================== */

(function () {
    'use strict';

    var SUPPORTED = ['en', 'it', 'fr', 'es', 'de'];
    var DEFAULT_LANG = 'en';
    var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    function $(sel, root) { return (root || document).querySelector(sel); }
    function $$(sel, root) { return Array.prototype.slice.call((root || document).querySelectorAll(sel)); }
    function store(key, val) { try { if (val === undefined) return localStorage.getItem(key); localStorage.setItem(key, val); } catch (e) { return null; } }
    function metaContent(name) { var el = $('meta[name="' + name + '"]'); return el ? el.getAttribute('content') : null; }

    // On pages rendered by build.py the text is already translated and only
    // the strings used at runtime ship with the page.
    var pageLang = metaContent('zl-page-lang');
    function strings() {
        var all = window.ZL_I18N || {};
        var lang = document.documentElement.lang;
        return all[lang] || all[DEFAULT_LANG] || { copy: 'Copy', copied: 'Copied' };
    }

    /* ---------------------------------------------------------------- Theme */

    function setTheme(theme) {
        document.documentElement.setAttribute('data-theme', theme);
        var meta = $('#theme-color-meta');
        if (meta) meta.setAttribute('content', theme === 'light' ? '#f4f2eb' : '#0b0c0a');
        var btn = $('#theme-toggle');
        if (btn) btn.setAttribute('aria-pressed', theme === 'light' ? 'true' : 'false');
        store('zl-theme', theme);
    }

    function initTheme() {
        var btn = $('#theme-toggle');
        if (!btn) return;
        btn.setAttribute('aria-pressed', document.documentElement.getAttribute('data-theme') === 'light' ? 'true' : 'false');
        btn.addEventListener('click', function () {
            var current = document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark';
            setTheme(current === 'light' ? 'dark' : 'light');
        });
    }

    /* ------------------------------------------------------------- Language */

    // Local preview only (plain index.html with the full i18n.js).
    function translate(lang) {
        var dict = window.ZL_I18N[lang] || window.ZL_I18N[DEFAULT_LANG];
        var fallback = window.ZL_I18N[DEFAULT_LANG];

        $$('[data-i18n]').forEach(function (el) {
            var key = el.getAttribute('data-i18n');
            var value = dict[key] !== undefined ? dict[key] : fallback[key];
            if (value === undefined) return;
            // These keys carry inline markup (<strong>, <code>); the rest is plain text.
            if (/_(p1|p2|html)$/.test(key) || /^a[0-9]+$/.test(key) || key === 'rel_notice') el.innerHTML = value;
            else el.textContent = value;
        });

        $$('[data-i18n-attr]').forEach(function (el) {
            // Format: "aria-label:nav_menu" — attribute name, then key.
            el.getAttribute('data-i18n-attr').split(',').forEach(function (pair) {
                var parts = pair.split(':');
                var key = parts[1].trim();
                var value = dict[key] !== undefined ? dict[key] : fallback[key];
                if (value !== undefined) el.setAttribute(parts[0].trim(), value);
            });
        });

        document.documentElement.lang = lang;
        var select = $('#lang-select');
        if (select) select.value = lang;
        store('zl-lang', lang);
    }

    function preferredLang() {
        var param = new URLSearchParams(window.location.search).get('lang');
        if (param && SUPPORTED.indexOf(param) !== -1) return param;
        var saved = store('zl-lang');
        if (saved && SUPPORTED.indexOf(saved) !== -1) return saved;
        var browser = (navigator.language || DEFAULT_LANG).slice(0, 2).toLowerCase();
        return SUPPORTED.indexOf(browser) !== -1 ? browser : DEFAULT_LANG;
    }

    function initLang() {
        var select = $('#lang-select');

        // Built site: one static page per language. Switching language means
        // going to that page, so every URL stays crawlable.
        if (pageLang) {
            var root = metaContent('zl-root') || '';
            var urlFor = function (lang) { return root + (lang === DEFAULT_LANG ? '' : lang + '/') + window.location.hash; };
            // Only the English root redirects, and only to a remembered or
            // browser language; language pages never bounce the visitor.
            if (pageLang === DEFAULT_LANG) {
                var wanted = preferredLang();
                if (wanted !== DEFAULT_LANG) { store('zl-lang', wanted); window.location.replace(urlFor(wanted)); return false; }
            }
            store('zl-lang', pageLang);
            if (select) {
                select.value = pageLang;
                select.addEventListener('change', function (e) {
                    store('zl-lang', e.target.value);
                    window.location.href = urlFor(e.target.value);
                });
            }
            return true;
        }

        translate(preferredLang());
        if (select) select.addEventListener('change', function (e) { translate(e.target.value); });
        return true;
    }

    /* ------------------------------------------------------------------ Nav */

    function initNav() {
        var nav = $('#nav');
        var progress = $('#nav-progress');
        var links = $('#nav-links');
        var menuBtn = $('#menu-btn');
        var maxScroll = 1;
        var ticking = false;
        var scrolled = null;

        function measure() { maxScroll = Math.max(1, document.documentElement.scrollHeight - window.innerHeight); }

        function frame() {
            ticking = false;
            var y = window.scrollY;
            var isScrolled = y > 8;
            if (nav && isScrolled !== scrolled) { nav.classList.toggle('is-scrolled', isScrolled); scrolled = isScrolled; }
            if (progress) progress.style.transform = 'scaleX(' + Math.min(1, y / maxScroll).toFixed(4) + ')';
        }

        function request() { if (!ticking) { ticking = true; requestAnimationFrame(frame); } }
        function remeasure() { measure(); request(); }

        window.addEventListener('scroll', request, { passive: true });
        window.addEventListener('resize', remeasure, { passive: true });
        window.addEventListener('load', remeasure);
        if ('ResizeObserver' in window) new ResizeObserver(remeasure).observe(document.body);
        requestAnimationFrame(remeasure);

        if (menuBtn && links) {
            var close = function () { links.classList.remove('is-open'); menuBtn.setAttribute('aria-expanded', 'false'); };
            menuBtn.addEventListener('click', function () {
                var open = links.classList.toggle('is-open');
                menuBtn.setAttribute('aria-expanded', open ? 'true' : 'false');
            });
            $$('a', links).forEach(function (a) { a.addEventListener('click', close); });
            document.addEventListener('keydown', function (e) {
                if (e.key === 'Escape' && links.classList.contains('is-open')) { close(); menuBtn.focus(); }
            });
        }
    }

    /* ----------------------------------------------------------------- Tabs */

    function initTabs() {
        $$('[data-tabs]').forEach(function (group) {
            var tabs = $$('[role="tab"]', group);
            var panels = $$('[role="tabpanel"]', group);

            function select(index) {
                tabs.forEach(function (tab, i) {
                    var active = i === index;
                    tab.setAttribute('aria-selected', active ? 'true' : 'false');
                    tab.tabIndex = active ? 0 : -1;
                });
                panels.forEach(function (panel, i) {
                    panel.hidden = i !== index;
                    if (i === index && !reduceMotion && panel.animate) {
                        panel.animate([{ opacity: 0, transform: 'translate3d(0, 6px, 0)' }, { opacity: 1, transform: 'none' }],
                            { duration: 420, easing: 'cubic-bezier(.16, 1, .3, 1)' });
                    }
                });
            }

            tabs.forEach(function (tab, i) {
                tab.tabIndex = i === 0 ? 0 : -1;
                tab.addEventListener('click', function () { select(i); });
                tab.addEventListener('keydown', function (e) {
                    var next = null;
                    if (e.key === 'ArrowRight') next = (i + 1) % tabs.length;
                    if (e.key === 'ArrowLeft') next = (i - 1 + tabs.length) % tabs.length;
                    if (e.key === 'Home') next = 0;
                    if (e.key === 'End') next = tabs.length - 1;
                    if (next !== null) { e.preventDefault(); select(next); tabs[next].focus(); }
                });
            });
        });
    }

    /* --------------------------------------------------------- Copy buttons */

    function initCopy() {
        $$('.copy-btn').forEach(function (btn) {
            btn.addEventListener('click', function () {
                var targetSel = btn.getAttribute('data-copy-target');
                var text = targetSel ? ($(targetSel) || {}).innerText : btn.getAttribute('data-copy');
                if (!text) return;
                text = text.replace(/^\s*\$\s?/gm, '').trim();

                var done = function () {
                    var dict = strings();
                    var label = $('[data-i18n="copy"]', btn);
                    btn.classList.add('is-copied');
                    if (label) label.textContent = dict.copied;
                    setTimeout(function () {
                        btn.classList.remove('is-copied');
                        if (label) label.textContent = dict.copy;
                    }, 1800);
                };

                if (navigator.clipboard && navigator.clipboard.writeText) {
                    navigator.clipboard.writeText(text).then(done).catch(function () {});
                } else {
                    var ta = document.createElement('textarea');
                    ta.value = text;
                    ta.style.position = 'fixed';
                    ta.style.opacity = '0';
                    document.body.appendChild(ta);
                    ta.select();
                    try { document.execCommand('copy'); done(); } catch (e) {}
                    document.body.removeChild(ta);
                }
            });
        });
    }

    /* -------------------------------------------------------- Terminal demo */

    // t: cmd (typed), bar (download progress), or an output class.
    var TERMINAL_SCRIPT = [
        { t: 'cmd',     text: 'zl install ripgrep' },
        { t: 'comment', text: '# no --from: every enabled source is queried' },
        { t: 'dim',     text: '  ? Found in 4 sources — pacman, apt, nix, github' },
        { t: 'ok',      text: '  ✓ Resolved 3 dependencies, no conflicts' },
        { t: 'bar',     text: '  [1/4] Downloading   ' },
        { t: 'ok',      text: '  [2/4] Verified SHA256 + GPG signature' },
        { t: 'ok',      text: '  [3/4] Patched 1 ELF  interpreter + RUNPATH' },
        { t: 'ok',      text: '  [4/4] Installed ripgrep 14.1.1   8 files · 5.1 MB' },
        { t: 'blank',   text: '' },
        { t: 'cmd',     text: 'rg --version' },
        { t: 'dim',     text: 'ripgrep 14.1.1' },
        { t: 'comment', text: '# a native binary — nothing wrapping it' }
    ];
    var BAR_WIDTH = 16;

    function line(cls) {
        var div = document.createElement('div');
        div.className = cls ? 'line ' + cls : 'line';
        return div;
    }

    function promptLine(text) {
        var div = line('');
        var p = document.createElement('span');
        p.className = 'prompt';
        p.textContent = '$';
        var t = document.createTextNode(' ' + text);
        div.appendChild(p);
        div.appendChild(t);
        return { el: div, text: t };
    }

    function staticLine(entry) {
        if (entry.t === 'cmd') return promptLine(entry.text).el;
        var div = line(entry.t === 'bar' ? 'dim' : entry.t);
        if (entry.t === 'blank') div.textContent = ' ';
        else if (entry.t === 'bar') div.textContent = entry.text + '█'.repeat(BAR_WIDTH) + '  2.4 MB/s';
        else div.textContent = entry.text;
        return div;
    }

    function initTerminal() {
        var body = $('#terminal-body');
        if (!body) return;

        if (reduceMotion || !('IntersectionObserver' in window)) {
            TERMINAL_SCRIPT.forEach(function (entry) { body.appendChild(staticLine(entry)); });
            return;
        }

        var cursor = document.createElement('span');
        cursor.className = 'cursor';

        var observer = new IntersectionObserver(function (entries) {
            if (!entries[0].isIntersecting) return;
            observer.disconnect();
            setTimeout(play, 450);
        }, { threshold: 0.3 });
        observer.observe(body);

        function play() {
            var i = 0;

            function next() {
                if (i >= TERMINAL_SCRIPT.length) { body.appendChild(cursor); return; }
                var entry = TERMINAL_SCRIPT[i++];
                if (entry.t === 'cmd') { type(entry); return; }
                if (entry.t === 'bar') { bar(entry); return; }
                body.appendChild(staticLine(entry));
                body.appendChild(cursor);
                setTimeout(next, entry.t === 'blank' ? 240 : 340);
            }

            // Commands are typed one character at a time, with a human jitter.
            function type(entry) {
                var l = promptLine('');
                body.appendChild(l.el);
                l.el.appendChild(cursor);
                var n = 0;
                (function key() {
                    if (n < entry.text.length) {
                        l.text.data = ' ' + entry.text.slice(0, ++n);
                        setTimeout(key, 36 + Math.random() * 48);
                    } else {
                        setTimeout(next, 420);
                    }
                })();
            }

            // The download bar fills block by block, then settles.
            function bar(entry) {
                var div = line('dim');
                var fill = document.createElement('span');
                fill.className = 'bar';
                var rest = document.createTextNode('');
                div.appendChild(document.createTextNode(entry.text));
                div.appendChild(fill);
                div.appendChild(rest);
                body.appendChild(div);
                body.appendChild(cursor);
                var k = 0;
                (function step() {
                    k++;
                    fill.textContent = '█'.repeat(k);
                    rest.data = '░'.repeat(BAR_WIDTH - k) + '  ' + (0.6 + k * 0.11).toFixed(1) + ' MB/s';
                    if (k < BAR_WIDTH) setTimeout(step, 55);
                    else { rest.data = '  2.4 MB/s'; setTimeout(next, 300); }
                })();
            }

            next();
        }
    }

    /* ---------------------------------------------------------- Stat counts */

    function initCounters() {
        var nums = $$('[data-count]');
        if (!nums.length || reduceMotion || !('IntersectionObserver' in window)) return;

        var observer = new IntersectionObserver(function (entries) {
            entries.forEach(function (entry) {
                if (!entry.isIntersecting) return;
                var el = entry.target;
                observer.unobserve(el);
                var target = parseInt(el.getAttribute('data-count'), 10);
                var start = performance.now();
                var duration = 1400;

                function tick(now) {
                    var p = Math.min((now - start) / duration, 1);
                    var eased = p === 1 ? 1 : 1 - Math.pow(2, -10 * p);   // expo-out, like the CSS
                    el.textContent = String(Math.round(target * eased));
                    if (p < 1) requestAnimationFrame(tick);
                }
                requestAnimationFrame(tick);
            });
        }, { threshold: 0.6 });

        nums.forEach(function (el) { observer.observe(el); });
    }

    /* -------------------------------------------------------------- Reveals */

    function initReveals() {
        $$('.stagger').forEach(function (group) {
            Array.prototype.forEach.call(group.children, function (child, i) { child.style.setProperty('--i', i); });
        });

        var items = $$('.reveal, .stagger');
        if (!items.length) return;
        if (reduceMotion || !('IntersectionObserver' in window)) {
            items.forEach(function (el) { el.classList.add('is-visible'); });
            return;
        }

        var observer = new IntersectionObserver(function (entries) {
            entries.forEach(function (entry) {
                if (!entry.isIntersecting) return;
                entry.target.classList.add('is-visible');
                observer.unobserve(entry.target);
            });
        }, { threshold: 0.12, rootMargin: '0px 0px -8% 0px' });

        items.forEach(function (el) { observer.observe(el); });
    }

    /* ----------------------------------------------------------------- Year */

    function initYear() {
        var el = $('#year');
        if (el) el.textContent = String(new Date().getFullYear());
    }

    /* ------------------------------------------------------------------ Boot */

    function boot() {
        if (initLang() === false) return;   // redirecting to another language
        initTheme();
        initNav();
        initTabs();
        initCopy();
        initTerminal();
        initCounters();
        initReveals();
        initYear();
    }

    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot);
    else boot();
})();

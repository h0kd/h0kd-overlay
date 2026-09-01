/**
 * Las tres páginas: /submit, /mod, /admin.
 *
 * Shells estáticos: el HTML no interpola NADA que venga de un usuario. Los
 * datos llegan por /api/* y se pintan con textContent, así el título de un Reel
 * o el nombre de un viewer no pueden inyectar markup. Es la razón por la que no
 * hay plantillas con `${}` de datos acá abajo.
 */

const CSS = `
/* ---------- tokens: dark-first ---------- */
  :root {
    color-scheme: dark;
    --bg: #0e0e10;
    --surface: #17171a;
    --surface-2: #1f1f23;
    --surface-3: #28282e;
    --border: #323239;
    --border-soft: #26262b;
    --text: #efeff1;
    --muted: #adadb8;
    --faint: #7a7a85;
    --accent: #9146ff;
    --accent-strong: #772ce8;
    --accent-soft: rgba(145, 70, 255, .15);
    --on-accent: #ffffff;
    --ok: #3dd68c;
    --ok-soft: rgba(61, 214, 140, .13);
    --bad: #ff5c7a;
    --bad-soft: rgba(255, 92, 122, .12);
    --warn: #f0b35c;
    --warn-soft: rgba(240, 179, 92, .13);
    --shadow: 0 10px 30px rgba(0, 0, 0, .35);
    --thumb-overlay: rgba(10, 8, 16, .55);
  }
  @media (prefers-color-scheme: light) {
    :root:not([data-theme="dark"]) {
      color-scheme: light;
      --bg: #f7f7f8;
      --surface: #ffffff;
      --surface-2: #efeff1;
      --surface-3: #e6e6ea;
      --border: #d9d9e0;
      --border-soft: #e8e8ec;
      --text: #1f1f23;
      --muted: #62626c;
      --faint: #9494a0;
      --accent: #772ce8;
      --accent-strong: #5c16c5;
      --accent-soft: rgba(119, 44, 232, .10);
      --on-accent: #ffffff;
      --ok: #1e9e63;
      --ok-soft: rgba(30, 158, 99, .12);
      --bad: #d8365b;
      --bad-soft: rgba(216, 54, 91, .10);
      --warn: #b87d1e;
      --warn-soft: rgba(184, 125, 30, .12);
      --shadow: 0 10px 30px rgba(60, 40, 110, .10);
      --thumb-overlay: rgba(20, 12, 40, .45);
    }
  }
  :root[data-theme="light"] {
    color-scheme: light;
    --bg: #f7f7f8;
    --surface: #ffffff;
    --surface-2: #efeff1;
    --surface-3: #e6e6ea;
    --border: #d9d9e0;
    --border-soft: #e8e8ec;
    --text: #1f1f23;
    --muted: #62626c;
    --faint: #9494a0;
    --accent: #772ce8;
    --accent-strong: #5c16c5;
    --accent-soft: rgba(119, 44, 232, .10);
    --on-accent: #ffffff;
    --ok: #1e9e63;
    --ok-soft: rgba(30, 158, 99, .12);
    --bad: #d8365b;
    --bad-soft: rgba(216, 54, 91, .10);
    --warn: #b87d1e;
    --warn-soft: rgba(184, 125, 30, .12);
    --shadow: 0 10px 30px rgba(60, 40, 110, .10);
    --thumb-overlay: rgba(20, 12, 40, .45);
  }
  * { box-sizing: border-box; }
  /* Al atributo hidden lo pisa cualquier regla que traiga su propio display, y
     en esta hoja hay varias (.reject-panel, .user-chip y .filter-bar son flex).
     Sin esto, esconder algo desde JS no esconde nada. */
  [hidden] { display: none !important; }
  body {
    background: var(--bg);
    color: var(--text);
    font-family: "Instrument Sans", "Segoe UI", system-ui, sans-serif;
    font-size: 15.5px;
    line-height: 1.5;
    min-height: 100vh;
  }
  h1, h2, h3 {
    font-family: "Bricolage Grotesque", "Instrument Sans", system-ui, sans-serif;
    text-wrap: balance;
    margin: 0;
  }
  button { font: inherit; cursor: pointer; }
  input, textarea, select { font: inherit; color: var(--text); }
  .mono { font-family: "Spline Sans Mono", ui-monospace, monospace; font-variant-numeric: tabular-nums; }
  /* ---------- topbar ---------- */
  .topbar {
    position: sticky; top: 0; z-index: 20;
    display: flex; align-items: center; gap: 20px;
    padding: 12px 24px;
    background: color-mix(in srgb, var(--bg) 82%, transparent);
    backdrop-filter: blur(12px);
    border-bottom: 1px solid var(--border-soft);
  }
  .logo { display: flex; align-items: center; gap: 10px; }
  /* El diseño tenía el conmutador de roles en el medio empujando el chip a la
     derecha. Sin él, el chip quedaba pegado al logo. */
  .user-chip { margin-left: auto; }
  .avatar img { width: 100%; height: 100%; border-radius: 50%; object-fit: cover; display: block; }
  .logo-mark {
    width: 34px; height: 34px; border-radius: 9px;
    background: linear-gradient(135deg, var(--accent-strong), var(--accent));
    display: grid; place-items: center; color: #fff;
    box-shadow: 0 4px 14px rgba(145, 70, 255, .35);
  }
  .logo-name {
    font-family: "Bricolage Grotesque", sans-serif;
    font-weight: 700; font-size: 19px; letter-spacing: -.02em;
  }
  .logo-name em { font-style: normal; color: var(--accent); }
  .user-chip {
    display: flex; align-items: center; gap: 10px;
    background: var(--surface-2); border: 1px solid var(--border);
    border-radius: 999px; padding: 5px 14px 5px 6px;
  }
  .user-chip .who { line-height: 1.2; }
  .user-chip .who b { font-size: 14px; display: block; }
  .user-chip .who span { font-size: 11.5px; color: var(--muted); display: flex; align-items: center; gap: 5px; }
  .twitch-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); display: inline-block; }
  .avatar {
    width: 32px; height: 32px; border-radius: 50%; flex: none;
    display: grid; place-items: center;
    font-weight: 600; font-size: 13px; color: #fff;
  }
  .avatar.lg { width: 46px; height: 46px; font-size: 17px; }
  .avatar.xs { width: 18px; height: 18px; font-size: 8px; }
  .mod-tag { display: inline-flex; align-items: center; gap: 5px; vertical-align: middle; position: relative; top: -1px; }
  .av-1 { background: linear-gradient(135deg, #7c3fe0, #b465ff); }
  .av-2 { background: linear-gradient(135deg, #d8365b, #ff7d5c); }
  .av-3 { background: linear-gradient(135deg, #148f8f, #3dd68c); }
  .av-4 { background: linear-gradient(135deg, #2a5cd6, #52b6ff); }
  .av-5 { background: linear-gradient(135deg, #b87d1e, #f0b35c); }
  .av-6 { background: linear-gradient(135deg, #8a2f7d, #e05fa8); }
  /* ---------- layout ---------- */
  main { max-width: 1060px; margin: 0 auto; padding: 30px 24px 80px; }
  .view-head { margin-bottom: 26px; }
  .view-head h1 { font-size: 30px; font-weight: 700; letter-spacing: -.02em; }
  .view-head p { color: var(--muted); margin: 6px 0 0; max-width: 60ch; }
  .card {
    background: var(--surface);
    border: 1px solid var(--border-soft);
    border-radius: 16px;
    /* En el diseño cada tarjeta traía su propio padding y la base no tenía
       ninguno, así que toda tarjeta nueva salía con el texto contra el borde.
       Las que ya tienen el suyo (.link-row, .stat) lo pisan más abajo. */
    padding: 18px 20px;
  }
  /* La tabla trae su espaciado en cada celda y va de borde a borde. */
  .card.flush { padding: 0; }
  /* ---------- tabs ---------- */
  .tabs { display: flex; gap: 6px; border-bottom: 1px solid var(--border-soft); margin-bottom: 22px; }
  /* La pastilla del agente viaja en la fila de pestañas, que lleva el borde
     inferior pegado. Sin esto se estira hasta el alto de la fila y queda
     apoyada sobre la línea. */
  .tabs > .pill { align-self: center; margin-bottom: 8px; }
  .tab {
    border: 0; background: transparent; color: var(--muted);
    padding: 10px 16px 12px; font-weight: 500; font-size: 15px;
    border-bottom: 2px solid transparent; margin-bottom: -1px;
    display: flex; align-items: center; gap: 8px;
  }
  .tab:hover { color: var(--text); }
  .tab.active { color: var(--text); border-bottom-color: var(--accent); font-weight: 600; }
  .tab .count {
    background: var(--accent-soft); color: var(--accent);
    font-size: 12px; font-weight: 600; padding: 1px 8px; border-radius: 999px;
  }
  /* ---------- pills / badges ---------- */
  .pill {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 12.5px; font-weight: 600;
    padding: 3px 11px; border-radius: 999px; white-space: nowrap;
  }
  .pill::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
  .pill.ok   { color: var(--ok);   background: var(--ok-soft); }
  .pill.bad  { color: var(--bad);  background: var(--bad-soft); }
  .pill.warn { color: var(--warn); background: var(--warn-soft); }
  /* ---------- link rows ---------- */
  .link-list { display: flex; flex-direction: column; gap: 12px; }
  .link-row {
    display: flex; gap: 16px; align-items: flex-start;
    padding: 14px;
  }
  .thumb {
    position: relative; flex: none;
    width: 104px; aspect-ratio: 9 / 14; border-radius: 10px;
    overflow: hidden;
  }
  .thumb .glow { position: absolute; inset: 0; }
  /* El diseno solo contemplaba el degradado; cuando hay miniatura de verdad
     tiene que llenar la caja igual, recortando en vez de deformar. La imagen
     viene de un CDN ajeno y puede tener cualquier proporcion. */
  .thumb img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  /* El horizontal es para clips 16:9; el vertical, para Reels y TikToks. */
  .thumb.wide { width: 168px; aspect-ratio: 16 / 9; }
  .t-1 { background: linear-gradient(160deg, #3b2a68 10%, #7f5adf 55%, #2a1b4d); }
  .t-2 { background: linear-gradient(200deg, #145049 5%, #2fae90 60%, #0d3a35); }
  .t-3 { background: linear-gradient(150deg, #6e2144 10%, #e0567f 65%, #471230); }
  .t-4 { background: linear-gradient(170deg, #1f3d73 5%, #4f8fd9 60%, #142a52); }
  .t-5 { background: linear-gradient(155deg, #6b4a12 10%, #d9a03f 60%, #4a3208); }
  .t-6 { background: linear-gradient(165deg, #204a26 10%, #56b361 65%, #143318); }
  .t-7 { background: linear-gradient(145deg, #52236e 5%, #a75ad9 60%, #33124a); }
  .t-8 { background: linear-gradient(185deg, #6e2121 10%, #d95a5a 65%, #471212); }
  .thumb .play {
    position: absolute; inset: 0; display: grid; place-items: center;
  }
  .thumb .play span {
    width: 34px; height: 34px; border-radius: 50%;
    background: var(--thumb-overlay); backdrop-filter: blur(2px);
    display: grid; place-items: center; color: #fff;
  }
  .thumb .dur {
    position: absolute; right: 6px; bottom: 6px;
    background: rgba(8, 6, 14, .78); color: #fff;
    font-size: 11px; padding: 2px 6px; border-radius: 6px;
  }
  .thumb .net {
    position: absolute; left: 6px; top: 6px;
    width: 24px; height: 24px; border-radius: 7px;
    display: grid; place-items: center; color: #fff;
  }
  .net.ig { background: radial-gradient(circle at 30% 110%, #fdc468 8%, #df4996 50%, #7238b8 95%); }
  .net.tt { background: #101014; box-shadow: 0 0 0 1px rgba(255,255,255,.14) inset; }
  .net.yt { background: #e62117; }
  .net.kp { background: #1f8a70; }
  .link-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; }
  .link-title { font-weight: 600; font-size: 16px; line-height: 1.35; }
  .link-meta { color: var(--muted); font-size: 13.5px; display: flex; flex-wrap: wrap; gap: 4px 14px; align-items: center; }
  .link-meta .net-name { display: inline-flex; align-items: center; gap: 6px; }
  /* El envoltorio que hace clickeable la miniatura y la URL: no tiene que
     parecer un link hasta que lo apuntás, o la fila se llena de color. */
  a.ir { color: inherit; text-decoration: none; }
  a.ir:hover { color: var(--accent); text-decoration: underline; }
  a.thumb { display: block; }
  a.thumb:hover { outline: 2px solid var(--accent); outline-offset: 2px; }
  .link-meta svg { width: 14px; height: 14px; }
  .byline { display: flex; align-items: center; gap: 9px; color: var(--muted); font-size: 13.5px; }
  .byline .avatar { width: 24px; height: 24px; font-size: 10.5px; }
  .byline b { color: var(--text); font-weight: 600; }
  .verdict { font-size: 13.5px; color: var(--muted); }
  .verdict b { color: var(--text); }
  .reason {
    margin-top: 2px; padding: 9px 12px; border-radius: 10px;
    background: var(--bad-soft); border: 1px solid color-mix(in srgb, var(--bad) 25%, transparent);
    color: var(--text); font-size: 13.5px;
  }
  .reason b { color: var(--bad); font-weight: 600; }
  .link-side { display: flex; flex-direction: column; align-items: flex-end; gap: 10px; flex: none; }
  /* ---------- buttons ---------- */
  .btn {
    display: inline-flex; align-items: center; justify-content: center; gap: 8px;
    border-radius: 10px; border: 1px solid var(--border);
    background: var(--surface-2); color: var(--text);
    padding: 8px 16px; font-weight: 600; font-size: 14px;
    transition: filter .15s ease, background .15s ease;
  }
  .btn:hover { background: var(--surface-3); }
  .btn.primary { background: var(--accent); border-color: transparent; color: var(--on-accent); }
  .btn.primary:hover { filter: brightness(1.08); }
  .btn.approve { background: var(--ok-soft); border-color: color-mix(in srgb, var(--ok) 35%, transparent); color: var(--ok); }
  .btn.reject  { background: var(--bad-soft); border-color: color-mix(in srgb, var(--bad) 30%, transparent); color: var(--bad); }
  .btn.ghost { background: transparent; border-color: transparent; color: var(--muted); }
  .btn.ghost:hover { color: var(--text); background: var(--surface-2); }
  .btn.sm { padding: 6px 12px; font-size: 13px; border-radius: 8px; }
  /* ---------- viewer: submit ---------- */
  .submit-card {
    padding: 26px 28px;
    background:
      radial-gradient(600px 220px at 15% -40%, var(--accent-soft), transparent 70%),
      var(--surface);
    margin-bottom: 30px;
  }
  .submit-card h2 { font-size: 21px; margin-bottom: 4px; }
  .submit-card .hint { color: var(--muted); font-size: 14px; margin: 0 0 16px; display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .hint .nets { display: inline-flex; gap: 8px; }
  .hint .nets .net { position: static; width: 22px; height: 22px; border-radius: 6px; display: grid; place-items: center; color: #fff; }
  .submit-form { display: flex; gap: 10px; }
  .submit-form input {
    flex: 1; background: var(--surface-2); border: 1px solid var(--border);
    border-radius: 10px; padding: 11px 15px; min-width: 0;
  }
  .submit-form input::placeholder { color: var(--faint); }
  .queue-note {
    margin-top: 14px; display: flex; align-items: center; gap: 10px;
    color: var(--warn); background: var(--warn-soft);
    border: 1px solid color-mix(in srgb, var(--warn) 25%, transparent);
    border-radius: 10px; padding: 10px 14px; font-size: 14px;
  }
  .queue-note b { font-weight: 600; }
  .queue-note .spacer { flex: 1; }
  .queue-note .pos { color: var(--text); font-size: 13px; }
  .section-title {
    display: flex; align-items: baseline; gap: 12px;
    margin: 0 0 14px;
  }
  .section-title h2 { font-size: 20px; }
  .section-title span { color: var(--faint); font-size: 13.5px; }
  /* ---------- mod queue ---------- */
  .mod-actions { display: flex; gap: 8px; }
  .reject-panel {
    margin-top: 12px; padding: 14px; border-radius: 12px;
    background: var(--surface-2); border: 1px solid var(--border);
    display: flex; flex-direction: column; gap: 10px;
  }
  .reject-panel label { font-size: 13px; color: var(--muted); font-weight: 500; }
  .reject-panel textarea {
    background: var(--surface); border: 1px solid var(--border); border-radius: 9px;
    padding: 9px 12px; resize: vertical; min-height: 58px; width: 100%;
  }
  .reject-panel .row { display: flex; gap: 8px; justify-content: flex-end; }
  .row-done { border-color: color-mix(in srgb, var(--ok) 30%, var(--border-soft)); }
  .row-done .decided-note { color: var(--ok); font-size: 13.5px; font-weight: 600; display: flex; align-items: center; gap: 7px; }
  .row-gone { border-color: color-mix(in srgb, var(--bad) 28%, var(--border-soft)); }
  .row-gone .decided-note { color: var(--bad); font-size: 13.5px; font-weight: 600; }
  /* ---------- admin ---------- */
  .stat-grid {
    display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: 12px; margin-bottom: 28px;
  }
  .stat {
    padding: 16px 18px;
  }
  .stat .label { font-size: 12px; text-transform: uppercase; letter-spacing: .08em; color: var(--faint); font-weight: 600; }
  .stat .value { font-size: 30px; font-weight: 700; font-family: "Bricolage Grotesque", sans-serif; margin-top: 4px; line-height: 1.1; }
  .stat .delta { font-size: 12.5px; color: var(--muted); margin-top: 4px; }
  .stat.ok .value { color: var(--ok); }
  .stat.bad .value { color: var(--bad); }
  .stat.warn .value { color: var(--warn); }
  .filter-bar {
    display: flex; flex-wrap: wrap; gap: 10px; align-items: center;
    margin-bottom: 16px;
  }
  .chip-group { display: flex; gap: 6px; background: var(--surface-2); border: 1px solid var(--border); border-radius: 999px; padding: 3px; }
  .chip {
    border: 0; background: transparent; color: var(--muted);
    font-size: 13px; font-weight: 500; padding: 5px 13px; border-radius: 999px;
  }
  .chip.active { background: var(--surface); color: var(--text); font-weight: 600; box-shadow: 0 1px 4px rgba(0,0,0,.25); }
  .chip:disabled { opacity: .35; pointer-events: none; }
  .filter-bar select {
    background: var(--surface-2); border: 1px solid var(--border);
    border-radius: 10px; padding: 7px 12px; font-size: 13.5px; color: var(--text);
  }
  table.data { width: 100%; border-collapse: collapse; font-size: 14.5px; }
  table.data th {
    text-align: left; font-size: 12px; text-transform: uppercase; letter-spacing: .07em;
    color: var(--faint); font-weight: 600; padding: 12px 16px;
    border-bottom: 1px solid var(--border-soft);
  }
  table.data td { padding: 12px 16px; border-bottom: 1px solid var(--border-soft); vertical-align: middle; }
  table.data tr:last-child td { border-bottom: 0; }
  table.data .num { text-align: right; }
  td.num, th.num { font-variant-numeric: tabular-nums; }
  .cell-user { display: flex; align-items: center; gap: 10px; font-weight: 600; }
  /* Solo la linea secundaria de la celda, no el avatar: el avatar tambien es un
     span y esta regla le pisaba color, tamano y display, dejandolo sin iniciales. */
  .cell-user div span { display: block; font-weight: 400; color: var(--faint); font-size: 12.5px; }
  .table-wrap { overflow-x: auto; }
  .t-ok { color: var(--ok); font-weight: 600; }
  .t-bad { color: var(--bad); font-weight: 600; }
  .t-warn { color: var(--warn); font-weight: 600; }
  .assign-row {
    display: flex; gap: 10px; padding: 16px; align-items: center;
    border-top: 1px solid var(--border-soft);
  }
  .assign-row input {
    flex: 1; background: var(--surface-2); border: 1px solid var(--border);
    border-radius: 10px; padding: 9px 14px; min-width: 0;
  }
  .assign-row input::placeholder { color: var(--faint); }
  .assign-box { position: relative; flex: 1; display: flex; }
  .assign-box input { flex: 1; }
  .assign-drop {
    position: absolute; top: calc(100% + 6px); left: 0; right: 0;
    background: var(--surface-2); border: 1px solid var(--border);
    border-radius: 12px; padding: 6px; box-shadow: var(--shadow);
    z-index: 15; max-height: 280px; overflow: auto;
    display: flex; flex-direction: column; gap: 2px;
  }
  .assign-drop .drop-label {
    font-size: 11px; text-transform: uppercase; letter-spacing: .08em;
    color: var(--faint); padding: 6px 10px 4px; font-weight: 600;
  }
  .assign-item {
    display: flex; align-items: center; gap: 10px; padding: 8px 10px;
    border-radius: 9px; background: transparent; border: 0;
    color: var(--text); font-weight: 600; width: 100%; text-align: left;
  }
  .assign-item:hover { background: var(--surface-3); }
  .assign-item .note { margin-left: auto; color: var(--faint); font-weight: 400; font-size: 12.5px; }
  .assign-empty { padding: 12px; color: var(--faint); font-size: 13.5px; text-align: center; }
  .row-actions { display: flex; gap: 8px; justify-content: flex-end; }
  tr.mod-revoked td { color: var(--faint); }
  tr.mod-revoked .cell-user { opacity: .55; }
  .revoked-note {
    display: flex; align-items: center; gap: 10px; justify-content: flex-end;
    color: var(--muted); font-size: 13px;
  }
  .profile-card { padding: 20px; margin-top: 16px; }
  .profile-head { display: flex; align-items: center; gap: 14px; margin-bottom: 16px; }
  .profile-head .who b { font-size: 18px; font-family: "Bricolage Grotesque", sans-serif; }
  .profile-head .who span { display: block; color: var(--muted); font-size: 13.5px; }
  .profile-stats { display: flex; gap: 22px; margin-left: auto; text-align: right; }
  .profile-stats .ps b { font-size: 20px; font-family: "Bricolage Grotesque", sans-serif; display: block; line-height: 1.1; }
  .profile-stats .ps span { font-size: 12px; color: var(--faint); text-transform: uppercase; letter-spacing: .06em; }
  /* ---------- toast ---------- */
  .toast {
    position: fixed; left: 50%; bottom: 28px; transform: translateX(-50%) translateY(8px);
    background: var(--surface-3); color: var(--text);
    border: 1px solid var(--border); border-radius: 12px;
    padding: 12px 18px; font-size: 14.5px; font-weight: 500;
    box-shadow: var(--shadow); opacity: 0; pointer-events: none;
    transition: opacity .25s ease, transform .25s ease;
    display: flex; align-items: center; gap: 10px; z-index: 50;
  }
  .toast.show { opacity: 1; transform: translateX(-50%) translateY(0); }
  .toast .ok-dot { color: var(--ok); }
  footer.proto-note {
    max-width: 1060px; margin: 0 auto; padding: 0 24px 40px;
    color: var(--faint); font-size: 13px; text-align: center;
  }
  @media (max-width: 720px) {
    .topbar { flex-wrap: wrap; padding: 10px 14px; }
    .user-chip .who span { display: none; }
    main { padding: 22px 14px 60px; }
    .link-row { gap: 12px; }
    .thumb { width: 84px; }
    .link-side { align-items: flex-start; }
    .link-row { flex-wrap: wrap; }
    .profile-stats { margin-left: 0; text-align: left; }
    .profile-head { flex-wrap: wrap; }
  }
  @media (prefers-reduced-motion: reduce) {
    * { transition: none !important; }
  }
  .net.tw { background: var(--accent); }
  /* ---------- formularios y estados vacios ----------
     El diseño no cubría la pestaña de ajustes, el emparejamiento ni los
     mensajes de "no hay nada". Va acá abajo, con los mismos tokens, para que
     no se note la costura. */
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 14px; }
  .grid label { display: block; color: var(--muted); font-size: 12.5px; margin-bottom: 6px; }
  input[type=number], input[type=text], select {
    width: 100%; background: var(--surface-2); border: 1px solid var(--border);
    color: var(--text); border-radius: 9px; padding: 9px 12px; font: inherit;
  }
  input[type=number]:focus, input[type=text]:focus, select:focus {
    outline: none; border-color: var(--accent);
  }
  label.sw { display: flex; align-items: center; gap: 10px; cursor: pointer; font-weight: 500; }
  label.sw input { width: auto; accent-color: var(--accent); }
  .mod-row {
    display: flex; align-items: center; gap: 12px; padding: 10px 0;
    border-top: 1px solid var(--border-soft);
  }
  .mod-row:first-child { border-top: none; }
  .mod-row .name { flex: 1; }
  .code-big {
    font-family: "Spline Sans Mono", ui-monospace, monospace;
    font-size: 30px; letter-spacing: .2em; font-weight: 500; text-align: center;
    padding: 16px; margin-top: 14px; border-radius: 12px;
    background: var(--surface-2); border: 1px solid var(--border);
  }
  .msg { margin-top: 12px; font-size: 13.5px; padding: 10px 13px; border-radius: 9px; display: none; }
  .msg.err { display: block; background: var(--bad-soft); color: var(--bad); }
  .msg.ok  { display: block; background: var(--ok-soft); color: var(--ok); }
  .empty { color: var(--muted); font-size: 14px; padding: 26px 0; text-align: center; }
  .waiting {
    font-size: 13.5px; color: var(--warn); background: var(--warn-soft);
    border-radius: 10px; padding: 11px 14px; margin-bottom: 14px; line-height: 1.5;
  }
  .stat-grid + .tabs { margin-top: 4px; }
  /* Tarjetas apiladas dentro de una pestana: sin esto se pegan y parecen una sola. */
  .tab-panel > .card + .card { margin-top: 14px; }

`;

/** Los <symbol> que usan todas las páginas. Markup fijo: nunca lleva datos. */
const ICONS = `<svg width="0" height="0" style="position:absolute" aria-hidden="true">
  <defs>
    <symbol id="ic-play" viewBox="0 0 24 24"><path fill="currentColor" d="M8 5.5v13l11-6.5z"/></symbol>
    <symbol id="ic-link" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" d="M10.5 13.5 13.5 10.5M8.6 15.4l-2.1 2.1a3 3 0 1 1-4.2-4.2l3.5-3.5a3 3 0 0 1 4.3 0M15.4 8.6l2.1-2.1a3 3 0 1 1 4.2 4.2l-3.5 3.5a3 3 0 0 1-4.3 0" transform="translate(1.1 1.1) scale(.9)"/></symbol>
    <symbol id="ic-ig" viewBox="0 0 24 24">
      <rect x="3" y="3" width="18" height="18" rx="5.2" fill="none" stroke="currentColor" stroke-width="2"/>
      <circle cx="12" cy="12" r="4.1" fill="none" stroke="currentColor" stroke-width="2"/>
      <circle cx="17.2" cy="6.8" r="1.3" fill="currentColor"/>
    </symbol>
    <symbol id="ic-tt" viewBox="0 0 24 24">
      <path fill="currentColor" d="M12.525.02c1.31-.02 2.61-.01 3.91-.02.08 1.53.63 3.09 1.75 4.17 1.12 1.11 2.7 1.62 4.24 1.79v4.03c-1.44-.05-2.89-.35-4.2-.97-.57-.26-1.1-.59-1.62-.93-.01 2.92.01 5.84-.02 8.75-.08 1.4-.54 2.79-1.35 3.94-1.31 1.92-3.58 3.17-5.91 3.21-1.43.08-2.86-.31-4.08-1.03-2.02-1.19-3.44-3.37-3.65-5.71-.02-.5-.03-1-.01-1.49.18-1.9 1.12-3.72 2.58-4.96 1.66-1.44 3.98-2.13 6.15-1.72.02 1.48-.04 2.96-.04 4.44-.99-.32-2.15-.23-3.02.37-.63.41-1.11 1.04-1.36 1.75-.21.51-.15 1.07-.14 1.61.24 1.64 1.82 3.02 3.5 2.87 1.12-.01 2.19-.66 2.77-1.61.19-.33.4-.67.41-1.06.1-1.79.06-3.57.07-5.36.01-4.03-.01-8.05.02-12.07z"/>
    </symbol>
    <symbol id="ic-yt" viewBox="0 0 24 24">
      <path fill="currentColor" d="M23.5 7.2s-.23-1.63-.94-2.35c-.9-.94-1.9-.95-2.36-1C16.88 3.6 12 3.6 12 3.6h-.01s-4.88 0-8.2.25c-.46.05-1.46.06-2.36 1C.72 5.57.5 7.2.5 7.2S.26 9.12.26 11.04v1.8c0 1.92.24 3.84.24 3.84s.23 1.63.94 2.35c.9.94 2.08.9 2.6 1 1.9.18 8 .24 8 .24s4.88-.01 8.2-.25c.46-.06 1.46-.07 2.36-1 .7-.73.94-2.36.94-2.36s.24-1.92.24-3.84v-1.8c0-1.92-.24-3.84-.24-3.84zM9.7 15.03V8.4l6.4 3.33-6.4 3.3z"/>
    </symbol>
    <symbol id="ic-tw" viewBox="0 0 24 24">
      <path fill="currentColor" d="M11.571 4.714h1.715v5.143H11.57zm4.715 0H18v5.143h-1.714zM6 0L1.714 4.286v15.428h5.143V24l4.286-4.286h3.428L22.286 12V0zm14.571 11.143l-3.428 3.428h-3.429l-3 3v-3H6.857V1.714h13.714z"/>
    </symbol>
    <symbol id="ic-kp" viewBox="0 0 24 24">
      <path fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" d="M7 4.5v15M17 4.5l-9 8.2M9.6 11.2 17 19.5"/>
    </symbol>
    <symbol id="ic-check" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" d="m4.5 12.5 5 5 10-11"/></symbol>
    <symbol id="ic-x" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" d="M6 6l12 12M18 6 6 18"/></symbol>
    <symbol id="ic-out" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4M10 8l-4 4 4 4M6 12h10"/></symbol>
  </defs></svg>`;

function page(title: string, body: string, script: string): string {
  return `<!doctype html>
<html lang="es"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="referrer" content="no-referrer">
<title>${title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,400;12..96,600;12..96,700&family=Instrument+Sans:ital,wght@0,400;0,500;0,600;1,400&family=Spline+Sans+Mono:wght@400;500&display=swap">
<style>${CSS}</style>
</head><body>
${ICONS}
<header class="topbar">
  <div class="logo">
    <div class="logo-mark"><svg width="19" height="19"><use href="#ic-link"/></svg></div>
    <div class="logo-name">Video <em>Requests</em></div>
  </div>
  <div class="user-chip" id="userChip" hidden>
    <div class="avatar" id="chipAvatar"></div>
    <div class="who">
      <b id="chipName"></b>
      <span><span class="twitch-dot"></span> Conectado con Twitch</span>
    </div>
    <button class="btn ghost sm" id="logoutBtn" title="Cerrar sesión" aria-label="Cerrar sesión"><svg width="16" height="16"><use href="#ic-out"/></svg></button>
  </div>
</header>
<main>${body}</main>
<div class="toast" id="toast"></div>
<script>${SHARED_JS}${script}</script>
</body></html>`;
}

/** Helpers compartidos por las tres páginas. */
const SHARED_JS = `
const $ = (s) => document.querySelector(s);
const ch = new URLSearchParams(location.search).get('ch') || '';
const SVGNS = 'http://www.w3.org/2000/svg';

function show(el, text, kind) { el.textContent = text; el.className = 'msg ' + (kind || ''); }
async function api(path, opts) {
  const res = await fetch(path, Object.assign({ credentials: 'same-origin' }, opts || {}));
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.error || ('Error ' + res.status));
  return body;
}
function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;   // nunca innerHTML con datos
  return n;
}
function login(to) { location.href = '/auth/login?ch=' + encodeURIComponent(ch) + '&to=' + encodeURIComponent(to); }

// Los iconos se arman con createElementNS en vez de innerHTML. El markup es
// fijo y no llevaria datos igual, pero asi no queda ni un innerHTML dando
// vueltas que alguien pueda copiar despues para pintar un titulo.
function icon(id, size) {
  const svg = document.createElementNS(SVGNS, 'svg');
  svg.setAttribute('width', size);
  svg.setAttribute('height', size);
  const use = document.createElementNS(SVGNS, 'use');
  use.setAttribute('href', '#' + id);
  svg.appendChild(use);
  return svg;
}

const NET = {
  tiktok:    { icono: 'ic-tt', clase: 'tt', nombre: 'TikTok' },
  instagram: { icono: 'ic-ig', clase: 'ig', nombre: 'Instagram' },
  youtube:   { icono: 'ic-yt', clase: 'yt', nombre: 'YouTube' },
  twitch:    { icono: 'ic-tw', clase: 'tw', nombre: 'Twitch' },
  kappa:     { icono: 'ic-kp', clase: 'kp', nombre: 'kappa.lol' },
};
function netBadge(plataforma) {
  const n = NET[plataforma];
  const span = el('span', 'net ' + (n ? n.clase : ''));
  if (n) { span.title = n.nombre; span.appendChild(icon(n.icono, 13)); }
  return span;
}
/** De la lista de hosts que manda el server a los nombres de plataforma. */
function plataformasDe(hosts) {
  const vistas = [];
  for (const h of hosts || []) {
    const p = h.indexOf('tiktok') >= 0 ? 'tiktok'
      : h.indexOf('instagram') >= 0 ? 'instagram'
      : h.indexOf('youtu') >= 0 ? 'youtube'
      : (h.indexOf('kappa') >= 0 ? 'kappa' : 'twitch');
    if (vistas.indexOf(p) < 0) vistas.push(p);
  }
  return vistas;
}

function iniciales(nombre) { return (nombre || '?').slice(0, 2).toUpperCase(); }
function colorAvatar(nombre) {
  let n = 0;
  for (const c of (nombre || '')) n += c.charCodeAt(0);
  return 'av-' + (n % 5 + 1);
}
function avatar(nombre, clase, foto) {
  const a = el('span', 'avatar ' + (clase || '') + ' ' + colorAvatar(nombre), iniciales(nombre));
  a.title = nombre || '';
  // Las iniciales de color quedan de red: si no hay foto, o si el CDN falla,
  // igual se distingue a la persona de un vistazo.
  if (foto) {
    const img = document.createElement('img');
    img.src = foto;
    img.alt = '';
    img.referrerPolicy = 'no-referrer';
    img.onerror = function () { img.remove(); a.textContent = iniciales(nombre); };
    a.textContent = '';
    a.appendChild(img);
  }
  return a;
}
function fillChip(me) {
  if (!me.login) return;
  const av = $('#chipAvatar');
  av.textContent = iniciales(me.login);
  av.className = 'avatar ' + colorAvatar(me.login);
  // La foto de Twitch si la hay; las iniciales quedan de red si el CDN falla o
  // si la sesión es vieja y todavía no la tiene guardada.
  if (me.pic) {
    const img = document.createElement('img');
    img.src = me.pic;
    img.alt = '';
    img.referrerPolicy = 'no-referrer';
    img.onerror = function () { img.remove(); av.textContent = iniciales(me.login); };
    av.textContent = '';
    av.appendChild(img);
  }
  $('#chipName').textContent = me.login;
  $('#userChip').hidden = false;
  $('#logoutBtn').onclick = async function () {
    try { await api('/auth/logout', { method: 'POST' }); } catch (e) {}
    location.reload();
  };
}

function hhmm(d) { return String(d.getHours()).padStart(2, '0') + ':' + String(d.getMinutes()).padStart(2, '0'); }
function diaDe(ts) { const d = new Date(ts); return d.getFullYear() + '-' + (d.getMonth() + 1) + '-' + d.getDate(); }
/** Tiempo relativo. Lo que importa es "cuando", no la precision. */
function hace(ts) {
  if (!ts) return '';
  const seg = Math.max(0, (Date.now() - ts) / 1000);
  if (seg < 90) return 'recién';
  if (seg < 3600) return 'hace ' + Math.round(seg / 60) + ' min';
  const d = new Date(ts);
  if (diaDe(ts) === diaDe(Date.now())) return 'hace ' + Math.round(seg / 3600) + ' h';
  if (diaDe(ts) === diaDe(Date.now() - 86400000)) return 'ayer, ' + hhmm(d);
  const dias = Math.floor(seg / 86400);
  if (dias < 7) return 'hace ' + dias + ' días';
  return d.toLocaleDateString('es', { day: 'numeric', month: 'short' }) + ', ' + hhmm(d);
}
function etiquetaDia(ts) {
  if (diaDe(ts) === diaDe(Date.now())) return 'Hoy';
  if (diaDe(ts) === diaDe(Date.now() - 86400000)) return 'Ayer';
  return new Date(ts).toLocaleDateString('es', { day: 'numeric', month: 'short' });
}
function mmss(s) {
  if (s == null) return '';
  const t = Math.round(s);
  return Math.floor(t / 60) + ':' + String(t % 60).padStart(2, '0');
}
function dur(s) { return s == null ? 'duración desconocida' : Math.round(s) + 's'; }
function urlCorta(u) {
  const limpio = String(u || '').replace('https://', '').replace('www.', '');
  return limpio.length > 34 ? limpio.slice(0, 33) + '…' : limpio;
}
/** Degradado estable por id, para que la miniatura no cambie de color al repintar. */
function tonoDe(id) {
  let n = 0;
  for (const c of String(id || '')) n += c.charCodeAt(0);
  return 't-' + (n % 8 + 1);
}
/** Miniatura: la imagen real si la hay, y si no un degradado con el play. */
function miniatura(it, ancha, href) {
  // Si va enlazada, la miniatura misma es el <a>. Envolverla en uno rompe su
  // caja: el ancla no toma la altura que le da el aspect-ratio y el recuadro se
  // sale de la tarjeta.
  const box = el(href ? 'a' : 'div', 'thumb' + (ancha ? ' wide' : ''));
  if (href) {
    box.href = href;
    box.target = '_blank';
    box.rel = 'noopener noreferrer';
    box.title = 'Ver el original en ' + it.platform;
  }
  if (it.thumbnail_url) {
    const img = document.createElement('img');
    img.src = it.thumbnail_url;
    img.alt = '';
    img.referrerPolicy = 'no-referrer';
    img.onerror = function () { img.remove(); box.appendChild(el('div', 'glow ' + tonoDe(it.id))); };
    box.appendChild(img);
  } else {
    box.appendChild(el('div', 'glow ' + tonoDe(it.id)));
  }
  const net = netBadge(it.platform);
  box.appendChild(net);
  const play = el('div', 'play');
  const cir = el('span');
  cir.appendChild(icon('ic-play', 16));
  play.appendChild(cir);
  box.appendChild(play);
  if (it.duration_seconds != null) box.appendChild(el('span', 'dur mono', mmss(it.duration_seconds)));
  return box;
}

function toast(msg) {
  const t = $('#toast');
  t.textContent = msg;
  t.classList.add('show');
  clearTimeout(toast._t);
  toast._t = setTimeout(function () { t.classList.remove('show'); }, 3200);
}
`;

// ── /submit ──────────────────────────────────────────────────────────────────

export function submitPage(): string {
  return page(
    'Mandar un video',
    `<div class="view-head">
       <h1>Mandá tu clip al stream</h1>
       <p id="chLine">Cargando…</p>
     </div>

     <div class="card" id="authCard" hidden>
       <p>Necesitás iniciar sesión con Twitch para mandar un link.</p>
       <div class="row" style="margin-top:14px"><button class="btn primary" id="loginBtn">Entrar con Twitch</button></div>
     </div>

     <div class="card submit-card" id="formCard" hidden>
       <h2>Nuevo link</h2>
       <p class="hint">Aceptamos <span class="nets" id="nets"></span> <span id="limites"></span></p>
       <div class="submit-form">
         <input type="url" id="url" placeholder="https://www.tiktok.com/@usuario/video/…" autocomplete="off" aria-label="Pegá tu link acá">
         <button class="btn primary" id="sendBtn">Enviar a la cola</button>
       </div>
       <div class="msg" id="msg"></div>
       <div class="queue-note" id="queueNote" hidden></div>
     </div>

     <div class="section-title" id="mineHead" hidden>
       <h2>Mis envíos</h2>
       <span id="mineCount"></span>
     </div>
     <div class="filter-bar" id="filterBar" hidden>
       <div class="chip-group" id="dayChips"></div>
     </div>
     <div class="link-list" id="mine"></div>`,
    `
// Cómo se ve cada estado: color de la pastilla, texto de la pastilla, y la
// línea de veredicto. El estado manda; el texto nunca se arma en el servidor.
const VISTA = {
  submitted:      ['warn', 'Pendiente',   'Leyendo el video…'],
  pending_review: ['warn', 'Pendiente',   'Esperando revisión de un mod…'],
  approved:       ['ok',   'Aprobado',    'Aprobado'],
  downloading:    ['ok',   'Aprobado',    'Preparándose…'],
  ready:          ['ok',   'Aprobado',    'Listo, esperando su turno en pantalla'],
  playing:        ['ok',   'En pantalla', 'Saliendo en el stream ahora'],
  played:         ['ok',   'Reproducido', 'Se reprodujo en el stream'],
  rejected:       ['bad',  'Rechazado',   'Rechazado'],
  rejected_auto:  ['bad',  'Rechazado',   'Rechazado sin pasar por un mod'],
  failed:         ['bad',  'Falló',       'No se pudo preparar el video'],
  cleared:        ['',     'Sin usar',    'La cola se limpió al terminar el stream'],
};

(async function () {
  let me;
  try { me = await api('/api/me?ch=' + encodeURIComponent(ch)); }
  catch (e) { $('#chLine').textContent = e.message; return; }

  fillChip(me);
  $('#chLine').textContent = me.channel_login
    ? (me.submissions_open
        ? 'Canal de ' + me.channel_login + '. Un mod lo revisa antes de que salga en pantalla.'
        : 'Canal de ' + me.channel_login + '. Los envíos están cerrados: el stream está offline.')
    : 'Canal desconocido';

  const nets = $('#nets');
  for (const p of plataformasDe(me.allowed_hosts)) nets.appendChild(netBadge(p));
  $('#limites').textContent = '· máximo ' + me.max_duration_seconds + ' segundos';

  if (!me.login) {
    $('#authCard').hidden = false;
    $('#loginBtn').onclick = () => login(location.pathname + location.search);
    return;
  }
  $('#formCard').hidden = false;

  let diaElegido = 'todos';
  let ultimos = [];

  function pintarChips() {
    const barra = $('#dayChips');
    barra.textContent = '';
    const dias = [];
    for (const it of ultimos) {
      const k = diaDe(it.created_at);
      if (!dias.some((d) => d.key === k)) dias.push({ key: k, label: etiquetaDia(it.created_at) });
    }
    if (dias.length < 2) { $('#filterBar').hidden = true; return; }
    $('#filterBar').hidden = false;
    const opciones = [{ key: 'todos', label: 'Todos' }].concat(dias);
    for (const o of opciones) {
      const b = el('button', 'chip' + (o.key === diaElegido ? ' active' : ''), o.label);
      b.onclick = () => { diaElegido = o.key; pintar(); };
      barra.appendChild(b);
    }
  }

  function pintar() {
    const box = $('#mine');
    box.textContent = '';
    pintarChips();

    const visibles = ultimos.filter((it) => diaElegido === 'todos' || diaDe(it.created_at) === diaElegido);
    if (!visibles.length) {
      box.appendChild(el('div', 'empty', ultimos.length ? 'Nada en ese día.' : 'Todavía no mandaste nada.'));
      return;
    }

    for (const it of visibles) {
      const v = VISTA[it.status] || ['', it.status, ''];
      const row = el('article', 'card link-row');
      // La miniatura y la URL llevan al original. Es lo primero que uno quiere
      // cuando ve su propio envío: volver a mirar qué mandó.
      const alOriginal = (nodo) => {
        const a = el('a', 'ir');
        a.href = it.source_url;
        a.target = '_blank';
        a.rel = 'noopener noreferrer';
        a.title = 'Ver el original en ' + it.platform;
        a.appendChild(nodo);
        return a;
      };
      row.appendChild(miniatura(it, it.platform !== 'instagram' && it.platform !== 'tiktok', it.source_url));

      const body = el('div', 'link-body');
      body.appendChild(el('div', 'link-title', it.title || urlCorta(it.source_url)));

      const meta = el('div', 'link-meta');
      meta.appendChild(alOriginal(el('span', 'mono', urlCorta(it.source_url))));
      meta.appendChild(el('span', null, 'Enviado ' + hace(it.created_at)));
      body.appendChild(meta);

      // Quién decidió se muestra hasta el final, no solo mientras está aprobado:
      // el que mandó el link quiere saber a quién agradecerle aunque el video
      // ya haya salido en pantalla.
      const APROBADOS_VISTA = ['approved', 'downloading', 'ready', 'playing', 'played'];
      const veredicto = el('div', 'verdict', v[2]);
      const rechazado = it.status === 'rejected';
      const aprobado = APROBADOS_VISTA.indexOf(it.status) >= 0;
      if (it.decided_by && (rechazado || aprobado)) {
        veredicto.textContent = it.status === 'approved' || rechazado
          ? v[2] + ' por '
          : v[2] + ' · aprobado por ';
        const tag = el('span', 'mod-tag');
        tag.appendChild(avatar(it.decided_by, 'xs', it.decided_pic));
        tag.appendChild(el('b', null, it.decided_by));
        veredicto.appendChild(tag);
        if (it.decided_at) veredicto.appendChild(el('span', null, ' · ' + hace(it.decided_at)));
      }
      body.appendChild(veredicto);

      // Dos cosas distintas que se ven igual: por qué lo rechazó un humano, y
      // por qué falló la máquina. El motivo del mod es el que más importa: es
      // la diferencia entre entender y volver a mandar lo mismo tres veces.
      const explicacion = it.decided_reason || it.error;
      if (explicacion) {
        const motivo = el('div', 'reason');
        // Un rechazo automático tiene motivo, no falla: el video estaba fuera de
        // las reglas del canal y nadie tuvo que mirarlo.
        const esMotivo = it.decided_reason || it.status === 'rejected_auto';
        motivo.appendChild(el('b', null, esMotivo ? 'Motivo: ' : 'Falló: '));
        motivo.appendChild(el('span', null, explicacion));
        body.appendChild(motivo);
      }
      row.appendChild(body);

      const side = el('div', 'link-side');
      side.appendChild(el('span', 'pill ' + v[0], v[1]));
      if (it.position) side.appendChild(el('span', 'mono', '#' + it.position + ' en cola'));
      row.appendChild(side);

      box.appendChild(row);
    }
  }

  async function refresh() {
    const data = await api('/api/mine?ch=' + encodeURIComponent(ch));
    ultimos = data.items;
    $('#mineHead').hidden = !ultimos.length;
    $('#mineCount').textContent = ultimos.length
      ? (ultimos.length === 1 ? '1 link' : ultimos.length + ' links')
      : '';

    const enJuego = ultimos.filter((it) => it.position);
    const nota = $('#queueNote');
    nota.textContent = '';
    nota.hidden = !enJuego.length;
    if (enJuego.length) {
      nota.appendChild(el('b', null, enJuego.length === 1
        ? 'Tenés 1 link en juego.'
        : 'Tenés ' + enJuego.length + ' links en juego.'));
      nota.appendChild(el('span', null, ' Te avisamos acá cuando un mod lo revise.'));
      nota.appendChild(el('span', 'spacer'));
      const proxima = Math.min.apply(null, enJuego.map((it) => it.position));
      nota.appendChild(el('span', 'pos mono', 'Posición en cola: #' + proxima));
    }
    pintar();
  }

  $('#sendBtn').onclick = async function () {
    const btn = this, url = $('#url').value;
    btn.disabled = true;
    try {
      await api('/api/submit', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ ch: ch, url: url }),
      });
      $('#url').value = '';
      show($('#msg'), '', '');
      toast('Tu link entró a la cola. Un mod lo va a revisar.');
      refresh();
    } catch (e) { show($('#msg'), e.message, 'err'); }
    btn.disabled = false;
  };
  $('#url').addEventListener('keydown', (e) => { if (e.key === 'Enter') $('#sendBtn').click(); });

  refresh();
  setInterval(refresh, 8000);
})();`,
  );
}

// ── /mod ─────────────────────────────────────────────────────────────────────

export function modPage(): string {
  return page(
    'Moderar pedidos',
    `<div class="view-head">
       <h1>Panel de moderación</h1>
       <p id="chLine">Revisá los links de la cola. Aprobar los manda a la lista del stream; rechazar avisa al viewer, con motivo si querés darlo.</p>
     </div>

     <div class="card" id="authCard" hidden>
       <p id="authMsg">Necesitás iniciar sesión con Twitch.</p>
       <div class="row" style="margin-top:14px"><button class="btn primary" id="loginBtn">Entrar con Twitch</button></div>
     </div>

     <div id="panel" hidden>
       <div class="tabs">
         <button class="tab active" data-tab="queue">Cola pendiente <span class="count" id="queueCount">0</span></button>
         <button class="tab" data-tab="history">Mi historial</button>
         <span style="flex:1"></span>
         <span class="pill" id="agentPill">agente: —</span>
       </div>

       <div id="tab-queue" class="tab-panel">
         <div id="waiting"></div>
         <div class="link-list" id="queue"></div>
       </div>

       <div id="tab-history" class="tab-panel" hidden>
         <div class="link-list" id="history"></div>
       </div>
     </div>`,
    `
const LABEL = {
  pending_review: 'esperando revisión', approved: 'aprobado', downloading: 'descargando',
  ready: 'listo', playing: 'reproduciendo', failed: 'falló', played: 'se reprodujo',
  rejected: 'rechazado', rejected_auto: 'rechazado automáticamente',
  cleared: 'se limpió al terminar el stream',
};
const PILL = {
  pending_review: 'warn', approved: 'ok', downloading: 'ok', ready: 'ok', playing: 'ok',
  played: 'ok', rejected: 'bad', rejected_auto: 'bad', failed: 'bad', cleared: '',
};

(async function () {
  let me;
  try { me = await api('/api/me?ch=' + encodeURIComponent(ch)); }
  catch (e) { $('#chLine').textContent = e.message; return; }
  fillChip(me);

  if (!me.login) {
    $('#authCard').hidden = false;
    $('#loginBtn').onclick = () => login(location.pathname + location.search);
    return;
  }
  if (me.role !== 'mod' && me.role !== 'broadcaster') {
    $('#authCard').hidden = false;
    $('#authMsg').textContent = 'Tu cuenta (' + me.login + ') no tiene permiso de moderación en este canal.';
    $('#loginBtn').textContent = 'Entrar con otra cuenta';
    $('#loginBtn').onclick = () => login(location.pathname + location.search);
    return;
  }
  $('#panel').hidden = false;
  $('#chLine').textContent = 'Canal de ' + me.channel_login + '. Aprobar manda el video a la lista del stream; rechazar avisa al viewer, con motivo si querés darlo.';

  // ── Pestañas ──
  const tabs = document.querySelectorAll('.tab');
  tabs.forEach((t) => t.onclick = () => {
    tabs.forEach((o) => o.classList.toggle('active', o === t));
    $('#tab-queue').hidden = t.dataset.tab !== 'queue';
    $('#tab-history').hidden = t.dataset.tab !== 'history';
    if (t.dataset.tab === 'history') cargarHistorial();
  });

  // ── Cola ──
  function tarjeta(it) {
    const row = el('article', 'card link-row');
    row.appendChild(miniatura(it, it.platform !== 'instagram' && it.platform !== 'tiktok'));

    const body = el('div', 'link-body');
    const byline = el('div', 'byline');
    byline.appendChild(avatar(it.submitter_login, null, it.submitter_pic));
    byline.appendChild(el('b', null, it.submitter_login));
    byline.appendChild(el('span', null, ' · ' + hace(it.created_at)));
    body.appendChild(byline);

    body.appendChild(el('div', 'link-title', it.title || urlCorta(it.source_url)));

    const meta = el('div', 'link-meta');
    meta.appendChild(el('span', 'mono', urlCorta(it.source_url)));
    meta.appendChild(el('span', null, dur(it.duration_seconds)));
    if (it.status !== 'pending_review') meta.appendChild(el('span', null, LABEL[it.status] || it.status));
    meta.appendChild(enlaceOriginal(it));
    body.appendChild(meta);

    if (it.error) {
      const motivo = el('div', 'reason');
      motivo.appendChild(el('b', null, 'Falló: '));
      motivo.appendChild(el('span', null, it.error));
      body.appendChild(motivo);
    }
    row.appendChild(body);

    const side = el('div', 'link-side');
    if (it.status === 'pending_review') {
      const acciones = el('div', 'mod-actions');
      const ok = el('button', 'btn approve');
      ok.appendChild(icon('ic-check', 15));
      ok.appendChild(el('span', null, ' Aprobar'));
      const no = el('button', 'btn reject');
      no.appendChild(icon('ic-x', 15));
      no.appendChild(el('span', null, ' Rechazar'));
      acciones.appendChild(ok);
      acciones.appendChild(no);
      side.appendChild(acciones);

      // El panel de motivo vive en el cuerpo, debajo del link, porque es donde
      // hay ancho para escribir. Aparece solo cuando el mod dice "rechazar".
      const panel = el('div', 'reject-panel');
      panel.hidden = true;
      panel.appendChild(el('label', null, 'Motivo del rechazo (opcional — el viewer lo va a ver)'));
      const texto = document.createElement('textarea');
      texto.placeholder = 'p. ej. contenido repetido, muy largo, fuera de tema…';
      texto.maxLength = 300;
      panel.appendChild(texto);
      const fila = el('div', 'row');
      const cancelar = el('button', 'btn ghost sm', 'Cancelar');
      const confirmar = el('button', 'btn reject sm', 'Rechazar');
      fila.appendChild(cancelar);
      fila.appendChild(confirmar);
      panel.appendChild(fila);
      body.appendChild(panel);

      ok.onclick = () => decidir(it.id, true, null, [ok, no]);
      no.onclick = () => { panel.hidden = false; acciones.hidden = true; texto.focus(); };
      cancelar.onclick = () => { panel.hidden = true; acciones.hidden = false; };
      confirmar.onclick = () => decidir(it.id, false, texto.value, [confirmar, cancelar]);
    } else {
      side.appendChild(el('span', 'pill ' + (PILL[it.status] || ''), LABEL[it.status] || it.status));
    }
    row.appendChild(side);
    return row;
  }

  function enlaceOriginal(it) {
    const a = el('a', null, 'ver el original');
    a.href = it.source_url;
    a.target = '_blank';
    a.rel = 'noopener noreferrer';
    return a;
  }

  function render(items, esperando) {
    const box = $('#queue');
    box.textContent = '';
    const aviso = $('#waiting');
    aviso.textContent = '';

    // Los pedidos en 'submitted' no son revisables todavía (no tienen título ni
    // duración), pero si el agente está caído se acumulan ahí. Decirlo evita
    // que el panel muestre "no hay nada" mientras la gente manda links al vacío.
    if (esperando > 0) {
      aviso.appendChild(el('div', 'waiting',
        esperando + (esperando === 1 ? ' pedido esperando' : ' pedidos esperando')
        + ' a que la app del streamer lea el video. No se pueden revisar hasta entonces.'));
    }
    $('#queueCount').textContent = items.filter((i) => i.status === 'pending_review').length;
    if (!items.length) { box.appendChild(el('div', 'empty', 'No hay nada en la cola.')); return; }
    for (const it of items) box.appendChild(tarjeta(it));
  }

  async function decidir(id, aprobado, motivo, btns) {
    btns.forEach((b) => b.disabled = true);
    try {
      await api('/api/decide', {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ ch: ch, item_id: id, approved: aprobado, reason: motivo || '' }),
      });
      toast(aprobado ? 'Aprobado. Va a la lista del stream.' : 'Rechazado. El viewer ya lo ve.');
    } catch (e) {
      toast(e.message);
      btns.forEach((b) => b.disabled = false);
    }
  }

  function agentPill(a) {
    const pill = $('#agentPill');
    if (!a || !a.online) { pill.className = 'pill bad'; pill.textContent = 'agente: desconectado'; return; }
    if (a.cookies_state === 'expired') { pill.className = 'pill warn'; pill.textContent = 'agente: cookies vencidas'; return; }
    pill.className = 'pill ok';
    pill.textContent = 'agente: conectado' + (a.now_playing ? ' · reproduciendo' : '');
  }

  // ── Mi historial ──
  let historialCargado = false;
  async function cargarHistorial() {
    const box = $('#history');
    if (!historialCargado) box.appendChild(el('div', 'empty', 'Cargando…'));
    let data;
    try { data = await api('/api/history?mine=1&ch=' + encodeURIComponent(ch)); }
    catch (e) { box.textContent = ''; box.appendChild(el('div', 'empty', e.message)); return; }
    historialCargado = true;
    box.textContent = '';
    if (!data.items.length) {
      box.appendChild(el('div', 'empty', 'Todavía no decidiste nada en este canal.'));
      return;
    }
    for (const it of data.items) {
      const row = el('article', 'card link-row');
      row.appendChild(miniatura(it, it.platform !== 'instagram' && it.platform !== 'tiktok'));
      const body = el('div', 'link-body');
      const byline = el('div', 'byline');
      byline.appendChild(avatar(it.submitter_login, null, it.submitter_pic));
      byline.appendChild(el('b', null, it.submitter_login));
      byline.appendChild(el('span', null, ' · ' + hace(it.created_at)));
      body.appendChild(byline);
      body.appendChild(el('div', 'link-title', it.title || urlCorta(it.source_url)));
      const meta = el('div', 'link-meta');
      meta.appendChild(el('span', 'mono', urlCorta(it.source_url)));
      meta.appendChild(el('span', null, (LABEL[it.status] || it.status) + ' · ' + hace(it.decided_at || it.created_at)));
      meta.appendChild(enlaceOriginal(it));
      body.appendChild(meta);
      if (it.decided_reason) {
        const motivo = el('div', 'reason');
        motivo.appendChild(el('b', null, 'Motivo: '));
        motivo.appendChild(el('span', null, it.decided_reason));
        body.appendChild(motivo);
      }
      row.appendChild(body);
      const side = el('div', 'link-side');
      side.appendChild(el('span', 'pill ' + (PILL[it.status] || ''), LABEL[it.status] || it.status));
      row.appendChild(side);
      box.appendChild(row);
    }
  }

  // ── WebSocket en vivo; si se cae, se cae a polling para no dejar la cola congelada. ──
  let ws = null, backoff = 1000;
  function connect() {
    ws = new WebSocket((location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + '/mod/ws?ch=' + encodeURIComponent(ch));
    ws.onopen = () => { backoff = 1000; };
    ws.onmessage = (ev) => {
      let m; try { m = JSON.parse(ev.data); } catch (_) { return; }
      if (m.type === 'queue') render(m.items, m.waiting || 0);
      else if (m.type === 'agent') agentPill(m);
    };
    ws.onclose = () => { setTimeout(connect, backoff); backoff = Math.min(30000, backoff * 2); };
    ws.onerror = () => {};
  }
  connect();
})();`,
  );
}

// ── /admin ───────────────────────────────────────────────────────────────────

export function adminPage(): string {
  return page(
    'Administrar Video Requests',
    `<div class="view-head">
       <h1>Administración del canal</h1>
       <p id="chLine">Cargando…</p>
     </div>

     <div class="card" id="authCard" hidden>
       <p id="authMsg">Necesitás iniciar sesión con Twitch.</p>
       <div class="row" style="margin-top:14px"><button class="btn primary" id="loginBtn">Entrar con Twitch</button></div>
     </div>

     <div id="panel" hidden>
       <div class="stat-grid" id="stats"></div>

       <div class="tabs">
         <button class="tab active" data-tab="queue">Cola <span class="count" id="queueCount">0</span></button>
         <button class="tab" data-tab="history">Historial global</button>
         <button class="tab" data-tab="mods">Moderadores <span class="count" id="modCount">0</span></button>
         <button class="tab" data-tab="viewers">Viewers</button>
         <button class="tab" data-tab="settings">Ajustes</button>
         <span style="flex:1"></span>
         <span class="pill" id="agentPill">agente: —</span>
       </div>

       <div id="tab-queue" class="tab-panel">
         <div id="waiting"></div>
         <div class="link-list" id="queue"></div>
       </div>

       <div id="tab-history" class="tab-panel" hidden>
         <div class="filter-bar" id="historyFilter" hidden>
           <span id="historyWho"></span>
           <button class="btn ghost sm" id="historyAll">Ver todo</button>
         </div>
         <div class="link-list" id="history"></div>
       </div>

       <div id="tab-mods" class="tab-panel" hidden>
         <p class="hint" style="margin:0 0 12px">Lista real de mods de tu canal, traída de Twitch. Marcá quiénes pueden entrar al panel de moderación; los números son lo que decidió cada uno acá.</p>
         <div class="card flush">
           <div class="table-wrap">
             <table class="data">
               <thead><tr>
                 <th>Moderador</th><th class="num">Acceso</th><th class="num">Aprobados</th>
                 <th class="num">Rechazados</th><th class="num">Última acción</th><th></th>
               </tr></thead>
               <tbody id="mods"></tbody>
             </table>
           </div>
         </div>
         <div class="filter-bar" id="botsBar" hidden>
           <span id="botsInfo"></span>
           <button class="btn ghost sm" id="botsToggle">Mostrar bots</button>
         </div>
       </div>

       <div id="tab-viewers" class="tab-panel" hidden>
         <div class="card flush">
           <div class="table-wrap">
             <table class="data">
               <thead><tr>
                 <th>Viewer</th><th class="num">Enviados</th><th class="num">Aprobados</th>
                 <th class="num">Rechazados</th><th class="num">En juego</th><th></th>
               </tr></thead>
               <tbody id="viewers"></tbody>
             </table>
           </div>
         </div>
       </div>

       <div id="tab-settings" class="tab-panel" hidden>
         <div class="card">
           <strong>Emparejar la app</strong>
           <p class="hint" style="margin:6px 0 12px">Generá un código y pegalo en la app de escritorio, en Video Requests.</p>
           <div class="row"><button class="btn primary" id="pairBtn">Generar código</button></div>
           <div class="code-big" id="pairCode" hidden></div>
         </div>
         <div class="card">
           <strong>Política de envío</strong>
           <label class="sw" style="margin-top:12px"><input type="checkbox" id="submissions_open"><span class="name">Envíos abiertos</span></label>
           <p class="hint" style="margin:6px 0 0">Normalmente lo maneja Twitch solo: se abren al arrancar el stream y se cierran un rato después de terminarlo. Tocá esto para probar sin estar en vivo, o para abrirlos a mano si Twitch no avisó.</p>
           <div class="grid" style="margin-top:14px">
             <div><label>Cooldown por usuario (s)</label><input type="number" id="cooldown_seconds"></div>
             <div><label>Máximo en cola por usuario</label><input type="number" id="max_pending_per_user"></div>
             <div><label>Duración máxima (s)</label><input type="number" id="max_duration_seconds"></div>
             <div><label>Tamaño máximo (MB)</label><input type="number" id="max_filesize_mb"></div>
             <div><label>Resolución máxima</label><select id="max_resolution"><option value="720">720</option><option value="1080">1080</option></select></div>
             <div><label>Gap entre videos (s)</label><input type="number" id="playback_gap_seconds"></div>
           </div>
           <div class="row" style="margin-top:14px"><button class="btn primary" id="saveBtn">Guardar</button></div>
           <div class="msg" id="msg"></div>
         </div>
       </div>
     </div>`,
    `
const LABEL = {
  pending_review: 'esperando revisión', approved: 'aprobado', downloading: 'descargando',
  ready: 'listo', playing: 'reproduciendo', failed: 'falló', played: 'se reprodujo',
  rejected: 'rechazado', rejected_auto: 'rechazado automáticamente',
  cleared: 'se limpió al terminar el stream',
};
const PILL = {
  pending_review: 'warn', approved: 'ok', downloading: 'ok', ready: 'ok', playing: 'ok',
  played: 'ok', rejected: 'bad', rejected_auto: 'bad', failed: 'bad', cleared: '',
};

(async function () {
  let me;
  try { me = await api('/api/me?ch=' + encodeURIComponent(ch)); }
  catch (e) { $('#chLine').textContent = e.message; return; }
  fillChip(me);

  if (!me.login) {
    // Sin sesión no hay nada que contar del canal, y dejar "Cargando…" para
    // siempre parece que la página se colgó.
    $('#chLine').textContent = 'Entrá con tu cuenta de Twitch para administrar la cola de tu canal.';
    $('#authCard').hidden = false;
    $('#loginBtn').onclick = () => login('/admin');
    return;
  }

  let data;
  try { data = await api('/api/admin/overview'); }
  catch (e) {
    $('#authCard').hidden = false;
    $('#authMsg').textContent = e.message;
    $('#loginBtn').textContent = 'Entrar con otra cuenta';
    $('#loginBtn').onclick = () => login('/admin');
    return;
  }
  $('#panel').hidden = false;
  // /admin no lleva ?ch= en la URL y /api/me sin ese parametro devuelve solo el
  // usuario. Acá el canal es, por definición, el del broadcaster que entró.
  const canal = ch || me.login;
  $('#chLine').textContent = 'Todo lo que pasa por la cola de ' + canal + ': estado general, historial, moderadores y actividad por viewer.';

  // ── Pestañas ──
  const tabs = document.querySelectorAll('.tab');
  const paneles = ['queue', 'history', 'mods', 'viewers', 'settings'];
  function abrir(cual) {
    tabs.forEach((o) => o.classList.toggle('active', o.dataset.tab === cual));
    for (const p of paneles) $('#tab-' + p).hidden = p !== cual;
    if (cual === 'history') cargarHistorial();
    if (cual === 'viewers') cargarViewers();
  }
  tabs.forEach((t) => t.onclick = () => abrir(t.dataset.tab));

  // ── Resumen ──
  function tarjetaStat(clase, etiqueta, valor, detalle) {
    const c = el('div', 'card stat ' + clase);
    c.appendChild(el('div', 'label', etiqueta));
    c.appendChild(el('div', 'value mono', valor));
    c.appendChild(el('div', 'delta', detalle));
    return c;
  }
  async function cargarStats() {
    let st;
    try { st = await api('/api/admin/stats'); } catch (e) { return; }
    const box = $('#stats');
    box.textContent = '';
    box.appendChild(tarjetaStat('warn', 'En cola ahora', String(st.en_cola),
      st.mas_viejo ? 'el más antiguo, ' + hace(st.mas_viejo) : 'la cola está vacía'));
    const delta = st.aprobados - st.aprobados_previos;
    box.appendChild(tarjetaStat('ok', 'Aprobados · 7 días', String(st.aprobados),
      (delta >= 0 ? '+' : '') + delta + ' vs. la semana anterior'));
    box.appendChild(tarjetaStat('bad', 'Rechazados · 7 días', String(st.rechazados),
      st.rechazados ? st.con_motivo + ' con motivo, ' + (st.rechazados - st.con_motivo) + ' sin motivo' : 'ninguno'));
    box.appendChild(tarjetaStat('', 'Tasa de aprobación',
      st.tasa === null ? '—' : st.tasa + '%',
      st.revisados ? st.revisados + ' links revisados' : 'todavía sin revisar nada'));
  }

  // ── Cola (el broadcaster también decide) ──
  function enlaceOriginal(it) {
    const a = el('a', null, 'ver el original');
    a.href = it.source_url; a.target = '_blank'; a.rel = 'noopener noreferrer';
    return a;
  }

  function tarjeta(it) {
    const row = el('article', 'card link-row');
    row.appendChild(miniatura(it, it.platform !== 'instagram' && it.platform !== 'tiktok'));
    const body = el('div', 'link-body');
    const byline = el('div', 'byline');
    byline.appendChild(avatar(it.submitter_login, null, it.submitter_pic));
    byline.appendChild(el('b', null, it.submitter_login));
    byline.appendChild(el('span', null, ' · ' + hace(it.created_at)));
    body.appendChild(byline);
    body.appendChild(el('div', 'link-title', it.title || urlCorta(it.source_url)));
    const meta = el('div', 'link-meta');
    meta.appendChild(el('span', 'mono', urlCorta(it.source_url)));
    meta.appendChild(el('span', null, dur(it.duration_seconds)));
    if (it.status !== 'pending_review') meta.appendChild(el('span', null, LABEL[it.status] || it.status));
    meta.appendChild(enlaceOriginal(it));
    body.appendChild(meta);
    if (it.error) {
      const m = el('div', 'reason');
      m.appendChild(el('b', null, 'Falló: '));
      m.appendChild(el('span', null, it.error));
      body.appendChild(m);
    }
    row.appendChild(body);

    const side = el('div', 'link-side');
    if (it.status === 'pending_review') {
      const acciones = el('div', 'mod-actions');
      const ok = el('button', 'btn approve');
      ok.appendChild(icon('ic-check', 15));
      ok.appendChild(el('span', null, ' Aprobar'));
      const no = el('button', 'btn reject');
      no.appendChild(icon('ic-x', 15));
      no.appendChild(el('span', null, ' Rechazar'));
      acciones.appendChild(ok); acciones.appendChild(no);
      side.appendChild(acciones);

      const panel = el('div', 'reject-panel');
      panel.hidden = true;
      panel.appendChild(el('label', null, 'Motivo del rechazo (opcional — el viewer lo va a ver)'));
      const texto = document.createElement('textarea');
      texto.placeholder = 'p. ej. contenido repetido, muy largo, fuera de tema…';
      texto.maxLength = 300;
      panel.appendChild(texto);
      const fila = el('div', 'row');
      const cancelar = el('button', 'btn ghost sm', 'Cancelar');
      const confirmar = el('button', 'btn reject sm', 'Rechazar');
      fila.appendChild(cancelar); fila.appendChild(confirmar);
      panel.appendChild(fila);
      body.appendChild(panel);

      ok.onclick = () => decidir(it.id, true, null, [ok, no]);
      no.onclick = () => { panel.hidden = false; acciones.hidden = true; texto.focus(); };
      cancelar.onclick = () => { panel.hidden = true; acciones.hidden = false; };
      confirmar.onclick = () => decidir(it.id, false, texto.value, [confirmar, cancelar]);
    } else {
      side.appendChild(el('span', 'pill ' + (PILL[it.status] || ''), LABEL[it.status] || it.status));
    }
    row.appendChild(side);
    return row;
  }

  async function decidir(id, aprobado, motivo, btns) {
    btns.forEach((b) => b.disabled = true);
    try {
      await api('/api/decide', {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ ch: canal, item_id: id, approved: aprobado, reason: motivo || '' }),
      });
      toast(aprobado ? 'Aprobado. Va a la lista del stream.' : 'Rechazado. El viewer ya lo ve.');
      cargarStats();
    } catch (e) {
      toast(e.message);
      btns.forEach((b) => b.disabled = false);
    }
  }

  function render(items, esperando) {
    const box = $('#queue');
    box.textContent = '';
    const aviso = $('#waiting');
    aviso.textContent = '';
    if (esperando > 0) {
      aviso.appendChild(el('div', 'waiting',
        esperando + (esperando === 1 ? ' pedido esperando' : ' pedidos esperando')
        + ' a que la app lea el video. No se pueden revisar hasta entonces.'));
    }
    $('#queueCount').textContent = items.filter((i) => i.status === 'pending_review').length;
    if (!items.length) { box.appendChild(el('div', 'empty', 'No hay nada en la cola.')); return; }
    for (const it of items) box.appendChild(tarjeta(it));
  }

  // ── Historial global ──
  let historial = [];
  // { tipo: 'viewer' | 'mod', quien } — el mismo historial sirve para "todo lo
  // que mandó fulano" y para "todo lo que decidió mengano".
  let filtro = null;
  async function cargarHistorial() {
    const box = $('#history');
    if (!historial.length) { box.textContent = ''; box.appendChild(el('div', 'empty', 'Cargando…')); }
    try {
      const data = await api('/api/history?ch=' + encodeURIComponent(canal));
      historial = data.items;
    } catch (e) {
      box.textContent = '';
      box.appendChild(el('div', 'empty', e.message));
      return;
    }
    pintarHistorial();
  }
  function pintarHistorial() {
    const box = $('#history');
    box.textContent = '';
    $('#historyFilter').hidden = !filtro;
    if (filtro) {
      $('#historyWho').textContent = filtro.tipo === 'mod'
        ? 'Mostrando lo que decidió ' + filtro.quien
        : 'Mostrando lo que mandó ' + filtro.quien;
    }
    const igual = (a, b) => String(a || '').toLowerCase() === String(b || '').toLowerCase();
    const visibles = !filtro ? historial
      : historial.filter((it) => filtro.tipo === 'mod'
          ? igual(it.decided_by, filtro.quien)
          : igual(it.submitter_login, filtro.quien));
    if (!visibles.length) {
      box.appendChild(el('div', 'empty', filtro
        ? (filtro.tipo === 'mod' ? 'Ese mod todavía no decidió nada.' : 'Ese viewer no tiene nada decidido todavía.')
        : 'Todavía no se decidió nada.'));
      return;
    }
    for (const it of visibles) {
      const row = el('article', 'card link-row');
      row.appendChild(miniatura(it, it.platform !== 'instagram' && it.platform !== 'tiktok'));
      const body = el('div', 'link-body');
      const byline = el('div', 'byline');
      byline.appendChild(avatar(it.submitter_login, null, it.submitter_pic));
      byline.appendChild(el('b', null, it.submitter_login));
      byline.appendChild(el('span', null, ' · ' + hace(it.created_at)));
      body.appendChild(byline);
      body.appendChild(el('div', 'link-title', it.title || urlCorta(it.source_url)));
      const meta = el('div', 'link-meta');
      meta.appendChild(el('span', 'mono', urlCorta(it.source_url)));
      meta.appendChild(el('span', null, (LABEL[it.status] || it.status)
        + (it.decided_by ? ' por ' + it.decided_by : '') + ' · ' + hace(it.decided_at || it.created_at)));
      meta.appendChild(enlaceOriginal(it));
      body.appendChild(meta);
      if (it.decided_reason) {
        const m = el('div', 'reason');
        m.appendChild(el('b', null, 'Motivo: '));
        m.appendChild(el('span', null, it.decided_reason));
        body.appendChild(m);
      }
      row.appendChild(body);
      const side = el('div', 'link-side');
      side.appendChild(el('span', 'pill ' + (PILL[it.status] || ''), LABEL[it.status] || it.status));
      row.appendChild(side);
      box.appendChild(row);
    }
  }
  $('#historyAll').onclick = () => { filtro = null; pintarHistorial(); };

  // ── Viewers ──
  let viewersCargados = false;
  async function cargarViewers() {
    const cuerpo = $('#viewers');
    let data;
    try { data = await api('/api/admin/viewers'); } catch (e) { return; }
    viewersCargados = true;
    cuerpo.textContent = '';
    if (!data.viewers.length) {
      const tr = el('tr');
      const td = el('td', 'empty', 'Todavía no mandó nadie.');
      td.colSpan = 6;
      tr.appendChild(td);
      cuerpo.appendChild(tr);
      return;
    }
    for (const v of data.viewers) {
      const tr = el('tr');
      const quien = el('td');
      const celda = el('div', 'cell-user');
      celda.appendChild(avatar(v.login, null, v.pic));
      celda.appendChild(el('div', null, v.login));
      quien.appendChild(celda);
      tr.appendChild(quien);
      tr.appendChild(el('td', 'num mono', String(v.enviados)));
      tr.appendChild(el('td', 'num mono t-ok', String(v.aprobados)));
      tr.appendChild(el('td', 'num mono t-bad', String(v.rechazados)));
      tr.appendChild(el('td', 'num mono t-warn', String(v.pendientes)));
      const accion = el('td', 'num');
      const btn = el('button', 'btn sm', 'Ver historial');
      btn.onclick = async () => { filtro = { tipo: 'viewer', quien: v.login }; abrir('history'); await cargarHistorial(); };
      accion.appendChild(btn);
      tr.appendChild(accion);
      cuerpo.appendChild(tr);
    }
  }

  // ── Moderadores ──
  // Los bots arrancan escondidos pero contados. Esconder sin decirlo es peor
  // que mostrar de más: si a alguien le falta un nombre, tiene dónde buscarlo.
  let verBots = false;
  const cuerpoMods = $('#mods');

  function pintarMods() {
    cuerpoMods.textContent = '';
    $('#modCount').textContent = data.mods.filter((m) => m.authorized).length;

    const bots = data.mods.filter((m) => m.es_bot);
    const barra = $('#botsBar');
    barra.hidden = !bots.length;
    if (bots.length) {
      $('#botsInfo').textContent = bots.length === 1
        ? '1 bot escondido de la lista'
        : bots.length + ' bots escondidos de la lista';
      $('#botsToggle').textContent = verBots ? 'Ocultar bots' : 'Mostrar bots';
    }

    const visibles = data.mods.filter((m) => verBots || !m.es_bot);
    if (!visibles.length) {
      const tr = el('tr');
      const td = el('td', 'empty', 'Tu canal no tiene mods en Twitch.');
      td.colSpan = 6;
      tr.appendChild(td);
      cuerpoMods.appendChild(tr);
      return;
    }

    for (const m of visibles) {
      const nombre = m.user_name || m.user_login;
      const tr = el('tr');

      const quien = el('td');
      const celda = el('div', 'cell-user');
      celda.appendChild(avatar(nombre, null, m.pic));
      const bloque = el('div');
      bloque.appendChild(document.createTextNode(nombre));
      if (!m.still_mod) bloque.appendChild(el('span', null, 'ya no es mod en Twitch'));
      else if (m.es_bot) bloque.appendChild(el('span', null, 'bot'));
      celda.appendChild(bloque);
      quien.appendChild(celda);
      tr.appendChild(quien);

      const acceso = el('td', 'num');
      // El dueño del canal no tiene casilla: su acceso no se puede quitar, y
      // una casilla marcada que no hace nada al tocarla es peor que ninguna.
      if (m.es_dueno) {
        acceso.appendChild(el('span', 'pill', 'sos vos'));
        tr.appendChild(acceso);
        tr.appendChild(el('td', 'num mono t-ok', String(m.aprobados)));
        tr.appendChild(el('td', 'num mono t-bad', String(m.rechazados)));
        tr.appendChild(el('td', 'num', m.ultima_accion ? hace(m.ultima_accion) : '—'));
        const propio = el('td', 'num');
        if (m.aprobados || m.rechazados) {
          const b = el('button', 'btn sm', 'Ver historial');
          b.onclick = async () => {
            filtro = { tipo: 'mod', quien: m.user_login };
            abrir('history');
            await cargarHistorial();
          };
          propio.appendChild(b);
        }
        tr.appendChild(propio);
        cuerpoMods.appendChild(tr);
        continue;
      }
      const cb = el('input'); cb.type = 'checkbox'; cb.checked = m.authorized;
      cb.title = 'Puede entrar al panel de moderación';
      cb.onchange = async () => {
        cb.disabled = true;
        try {
          await api('/api/admin/mods', { method: 'POST', headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ user_id: m.user_id, login: m.user_login, authorized: cb.checked }) });
          m.authorized = cb.checked;
          $('#modCount').textContent = data.mods.filter((x) => x.authorized).length;
        } catch (e) { toast(e.message); cb.checked = !cb.checked; }
        cb.disabled = false;
      };
      acceso.appendChild(cb);
      tr.appendChild(acceso);

      tr.appendChild(el('td', 'num mono t-ok', String(m.aprobados)));
      tr.appendChild(el('td', 'num mono t-bad', String(m.rechazados)));
      tr.appendChild(el('td', 'num', m.ultima_accion ? hace(m.ultima_accion) : '—'));

      const accion = el('td', 'num');
      if (m.aprobados || m.rechazados) {
        const btn = el('button', 'btn sm', 'Ver historial');
        btn.onclick = async () => {
          filtro = { tipo: 'mod', quien: m.user_login };
          abrir('history');
          await cargarHistorial();
        };
        accion.appendChild(btn);
      }
      tr.appendChild(accion);
      cuerpoMods.appendChild(tr);
    }
  }

  $('#botsToggle').onclick = () => { verBots = !verBots; pintarMods(); };
  pintarMods();

  // ── Ajustes ──
  const KEYS = ['cooldown_seconds','max_pending_per_user','max_duration_seconds','max_filesize_mb','max_resolution','playback_gap_seconds'];
  for (const k of KEYS) $('#' + k).value = data.settings[k];
  $('#submissions_open').checked = !!data.settings.submissions_open;

  $('#saveBtn').onclick = async function () {
    this.disabled = true;
    const patch = {};
    for (const k of KEYS) patch[k] = $('#' + k).value;
    patch.submissions_open = $('#submissions_open').checked;
    try {
      await api('/api/admin/settings', { method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify(patch) });
      show($('#msg'), 'Guardado.', 'ok');
    } catch (e) { show($('#msg'), e.message, 'err'); }
    this.disabled = false;
  };

  $('#pairBtn').onclick = async function () {
    this.disabled = true;
    try {
      const r = await api('/api/admin/pair', { method: 'POST' });
      const caja = $('#pairCode');
      caja.hidden = false;
      caja.textContent = r.code;
      toast('El código vale ' + Math.round(r.expires_in / 60) + ' minutos y se usa una sola vez.');
    } catch (e) { toast(e.message); }
    this.disabled = false;
  };

  function agentPill(a) {
    const pill = $('#agentPill');
    if (!a || !a.online) { pill.className = 'pill bad'; pill.textContent = 'agente: desconectado'; return; }
    if (a.cookies_state === 'expired') { pill.className = 'pill warn'; pill.textContent = 'agente: cookies vencidas'; return; }
    pill.className = 'pill ok';
    pill.textContent = 'agente: conectado' + (a.now_playing ? ' · reproduciendo' : '');
  }

  // La cola del admin es la misma que la de los mods: mismo socket, misma vista.
  let ws = null, backoff = 1000;
  function connect() {
    ws = new WebSocket((location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host
      + '/mod/ws?ch=' + encodeURIComponent(canal));
    ws.onopen = () => { backoff = 1000; };
    ws.onmessage = (ev) => {
      let m; try { m = JSON.parse(ev.data); } catch (_) { return; }
      if (m.type === 'queue') { render(m.items, m.waiting || 0); cargarStats(); }
      else if (m.type === 'agent') agentPill(m);
    };
    ws.onclose = () => { setTimeout(connect, backoff); backoff = Math.min(30000, backoff * 2); };
    ws.onerror = () => {};
  }
  connect();
  cargarStats();
})();`,
  );
}

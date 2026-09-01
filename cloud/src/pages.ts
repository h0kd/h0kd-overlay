/**
 * Las tres páginas: /submit, /mod, /admin.
 *
 * Shells estáticos: el HTML no interpola NADA que venga de un usuario. Los
 * datos llegan por /api/* y se pintan con textContent, así el título de un Reel
 * o el nombre de un viewer no pueden inyectar markup. Es la razón por la que no
 * hay plantillas con `${}` de datos acá abajo.
 */

const CSS = `
:root{--bg:#0e0e10;--s1:#18181b;--s2:#1f1f23;--border:#2a2a2d;--accent:#9147ff;
--accentH:#772ce8;--text:#efeff1;--muted:#adadb8;--danger:#c0392b;--green:#00ba6c}
*{margin:0;padding:0;box-sizing:border-box}
body{background:var(--bg);color:var(--text);font:14px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif;
padding:32px 20px;display:flex;justify-content:center}
.wrap{width:100%;max-width:720px}
h1{font-size:20px;margin-bottom:4px}
h1 span{color:var(--accent)}
.sub{color:var(--muted);font-size:13px;margin-bottom:24px}
.card{background:var(--s1);border:1px solid var(--border);border-radius:10px;padding:18px;margin-bottom:16px}
.row{display:flex;gap:10px;align-items:center}
input[type=text],input[type=number],select{background:var(--s2);border:1px solid var(--border);
color:var(--text);border-radius:6px;padding:9px 11px;font:inherit;flex:1;min-width:0}
input:focus,select:focus{outline:none;border-color:var(--accent)}
button{background:var(--accent);color:#fff;border:none;border-radius:6px;padding:9px 16px;
font:inherit;font-weight:600;cursor:pointer}
button:hover:not(:disabled){background:var(--accentH)}
button:disabled{opacity:.45;cursor:default}
button.ghost{background:var(--s2);border:1px solid var(--border);color:var(--text)}
button.ok{background:var(--green);color:#04150c}
button.no{background:var(--danger)}
a{color:var(--accent)}
.msg{margin-top:12px;font-size:13px;padding:9px 12px;border-radius:6px;display:none}
.msg.err{display:block;background:#3a1212;color:#ffbcbc}
.msg.ok{display:block;background:#00301a;color:#8ff0c0}
.item{display:flex;gap:12px;padding:12px 0;border-top:1px solid var(--border);align-items:flex-start}
.item:first-child{border-top:none}
.thumb{width:72px;height:128px;object-fit:cover;border-radius:6px;background:var(--s2);flex:none}
.thumb.wide{width:128px;height:72px}
.meta{flex:1;min-width:0}
.meta .t{font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.meta .d{color:var(--muted);font-size:12px;margin-top:2px}
.actions{display:flex;gap:8px;flex-direction:column}
.pill{font-size:11px;padding:2px 8px;border-radius:999px;background:var(--s2);color:var(--muted)}
.pill.on{background:#00301a;color:var(--green)}
.pill.warn{background:#3a2a00;color:#f0a000}
.pill.err{background:#3a1212;color:#ffbcbc}
.empty{color:var(--muted);font-size:13px;padding:18px 0;text-align:center}
.waiting{font-size:12.5px;color:#f0a000;background:#3a2a00;border-radius:6px;
padding:9px 12px;margin-bottom:12px;line-height:1.5}
.mod-row{display:flex;align-items:center;gap:10px;padding:9px 0;border-top:1px solid var(--border)}
.mod-row:first-child{border-top:none}
.mod-row .name{flex:1}
label.sw{display:flex;align-items:center;gap:8px;cursor:pointer}
.grid{display:grid;grid-template-columns:1fr 1fr;gap:12px}
.grid label{display:block;color:var(--muted);font-size:12px;margin-bottom:4px}
code{background:var(--s2);padding:2px 6px;border-radius:4px;font-size:12px}
.code-big{font-size:28px;letter-spacing:.18em;font-weight:700;text-align:center;padding:14px;
background:var(--s2);border-radius:8px;margin-top:12px}
`;

function page(title: string, body: string, script: string): string {
  return `<!doctype html>
<html lang="es"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="referrer" content="no-referrer">
<title>${title}</title>
<style>${CSS}</style>
</head><body><div class="wrap">${body}</div>
<script>${SHARED_JS}${script}</script>
</body></html>`;
}

/** Helpers compartidos por las tres páginas. */
const SHARED_JS = `
const $ = (s) => document.querySelector(s);
const ch = new URLSearchParams(location.search).get('ch') || '';
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
function dur(s) { return s == null ? 'duración desconocida' : Math.round(s) + 's'; }
function login(to) { location.href = '/auth/login?ch=' + encodeURIComponent(ch) + '&to=' + encodeURIComponent(to); }
`;

// ── /submit ──────────────────────────────────────────────────────────────────

export function submitPage(): string {
  return page(
    'Mandar un video',
    `<h1>Mandar un <span>video</span></h1>
     <p class="sub" id="chLine">Cargando…</p>
     <div class="card" id="authCard" style="display:none">
       <p>Necesitás iniciar sesión con Twitch para mandar un link.</p>
       <div class="row" style="margin-top:12px"><button id="loginBtn">Entrar con Twitch</button></div>
     </div>
     <div class="card" id="formCard" style="display:none">
       <div class="row">
         <input type="text" id="url" placeholder="https://www.instagram.com/reel/..." autocomplete="off">
         <button id="sendBtn">Mandar</button>
       </div>
       <p class="sub" style="margin:10px 0 0" id="hosts"></p>
       <div class="msg" id="msg"></div>
     </div>
     <div class="card" id="mineCard" style="display:none">
       <h1 style="font-size:15px;margin-bottom:12px">Tus pedidos</h1>
       <div id="mine"></div>
     </div>`,
    `
(async function () {
  let me;
  try { me = await api('/api/me?ch=' + encodeURIComponent(ch)); }
  catch (e) { $('#chLine').textContent = e.message; return; }

  $('#chLine').textContent = me.channel_login
    ? ('Canal: ' + me.channel_login + (me.submissions_open ? '' : ' — envíos cerrados (stream offline)'))
    : 'Canal desconocido';
  $('#hosts').textContent = 'Se aceptan links de: ' + me.allowed_hosts.join(', ') + '. Máximo ' + me.max_duration_seconds + ' segundos.';

  if (!me.login) { $('#authCard').style.display = ''; $('#loginBtn').onclick = () => login(location.pathname + location.search); return; }
  $('#formCard').style.display = '';
  $('#mineCard').style.display = '';

  async function refresh() {
    const data = await api('/api/mine?ch=' + encodeURIComponent(ch));
    const box = $('#mine'); box.textContent = '';
    if (!data.items.length) { box.appendChild(el('div', 'empty', 'Todavía no mandaste nada.')); return; }
    for (const it of data.items) {
      const row = el('div', 'item');
      const meta = el('div', 'meta');
      meta.appendChild(el('div', 't', it.title || it.source_url));
      meta.appendChild(el('div', 'd', it.status_label + (it.error ? ' — ' + it.error : '')));
      row.appendChild(meta);
      box.appendChild(row);
    }
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
      show($('#msg'), '¡Listo! Un mod lo va a revisar.', 'ok');
      refresh();
    } catch (e) { show($('#msg'), e.message, 'err'); }
    btn.disabled = false;
  };

  refresh();
  setInterval(refresh, 8000);
})();`,
  );
}

// ── /mod ─────────────────────────────────────────────────────────────────────

export function modPage(): string {
  return page(
    'Moderar pedidos',
    `<h1>Moderar <span>pedidos</span></h1>
     <p class="sub" id="chLine">Cargando…</p>
     <div class="card" id="authCard" style="display:none">
       <p id="authMsg">Necesitás iniciar sesión con Twitch.</p>
       <div class="row" style="margin-top:12px"><button id="loginBtn">Entrar con Twitch</button></div>
     </div>
     <div class="card" id="queueCard" style="display:none">
       <div class="row" style="justify-content:space-between;margin-bottom:8px">
         <strong>Cola</strong><span class="pill" id="agentPill">agente: —</span>
       </div>
       <div id="queue"></div>
     </div>`,
    `
(async function () {
  let me;
  try { me = await api('/api/me?ch=' + encodeURIComponent(ch)); }
  catch (e) { $('#chLine').textContent = e.message; return; }
  $('#chLine').textContent = 'Canal: ' + (me.channel_login || '?');

  if (!me.login) { $('#authCard').style.display = ''; $('#loginBtn').onclick = () => login(location.pathname + location.search); return; }
  if (me.role !== 'mod' && me.role !== 'broadcaster') {
    $('#authCard').style.display = '';
    $('#authMsg').textContent = 'Tu cuenta (' + me.login + ') no tiene permiso de moderación en este canal.';
    $('#loginBtn').textContent = 'Entrar con otra cuenta';
    $('#loginBtn').onclick = () => login(location.pathname + location.search);
    return;
  }
  $('#queueCard').style.display = '';

  const LABEL = { pending_review: 'esperando revisión', approved: 'aprobado', downloading: 'descargando',
                  ready: 'listo', playing: 'reproduciendo', failed: 'falló' };

  function render(items, waiting) {
    const box = $('#queue'); box.textContent = '';
    // Los pedidos en 'submitted' no son revisables todavía (no tienen título ni
    // duración), pero si el agente está caído se acumulan ahí. Decirlo evita
    // que el panel muestre "no hay nada" mientras la gente manda links al vacío.
    if (waiting > 0) {
      box.appendChild(el('div', 'waiting',
        waiting + (waiting === 1 ? ' pedido esperando' : ' pedidos esperando')
        + ' a que la app del streamer lea el video. No se pueden revisar hasta entonces.'));
    }
    if (!items.length) { box.appendChild(el('div', 'empty', 'No hay nada en la cola.')); return; }
    for (const it of items) {
      const row = el('div', 'item');
      if (it.thumbnail_url) {
        const img = el('img', 'thumb' + (it.platform === 'instagram' ? '' : ' wide'));
        img.src = it.thumbnail_url; img.alt = ''; img.referrerPolicy = 'no-referrer';
        row.appendChild(img);
      }
      const meta = el('div', 'meta');
      meta.appendChild(el('div', 't', it.title || '(sin título)'));
      meta.appendChild(el('div', 'd', 'de ' + it.submitter_login + ' · ' + dur(it.duration_seconds) + ' · ' + (LABEL[it.status] || it.status) + (it.error ? ' — ' + it.error : '')));
      const link = el('a', null, 'Ver el original en ' + it.platform);
      link.href = it.source_url; link.target = '_blank'; link.rel = 'noopener noreferrer';
      const p = el('div', 'd'); p.appendChild(link);
      meta.appendChild(p);
      row.appendChild(meta);

      if (it.status === 'pending_review') {
        const actions = el('div', 'actions');
        const ok = el('button', 'ok', 'Aprobar');
        const no = el('button', 'no', 'Rechazar');
        ok.onclick = () => decide(it.id, true, [ok, no]);
        no.onclick = () => decide(it.id, false, [ok, no]);
        actions.appendChild(ok); actions.appendChild(no);
        row.appendChild(actions);
      }
      box.appendChild(row);
    }
  }

  async function decide(id, approved, btns) {
    btns.forEach(b => b.disabled = true);
    try {
      await api('/api/decide', { method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ ch: ch, item_id: id, approved: approved }) });
    } catch (e) { alert(e.message); btns.forEach(b => b.disabled = false); }
  }

  function agentPill(a) {
    const pill = $('#agentPill');
    if (!a || !a.online) { pill.className = 'pill err'; pill.textContent = 'agente: desconectado'; return; }
    if (a.cookies_state === 'expired') { pill.className = 'pill warn'; pill.textContent = 'agente: cookies vencidas'; return; }
    pill.className = 'pill on';
    pill.textContent = 'agente: conectado' + (a.now_playing ? ' · reproduciendo' : '');
  }

  // WebSocket en vivo; si se cae, se cae a polling para no dejar la cola congelada.
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
    `<h1>Video <span>Requests</span> — admin</h1>
     <p class="sub" id="chLine">Cargando…</p>
     <div class="card" id="authCard" style="display:none">
       <p id="authMsg">Necesitás iniciar sesión con Twitch.</p>
       <div class="row" style="margin-top:12px"><button id="loginBtn">Entrar con Twitch</button></div>
     </div>
     <div id="panel" style="display:none">
       <div class="card">
         <strong>Emparejar la app</strong>
         <p class="sub" style="margin:6px 0 0">Generá un código y pegalo en la app de escritorio, en Video Requests.</p>
         <div class="row" style="margin-top:12px"><button id="pairBtn">Generar código</button></div>
         <div class="code-big" id="pairCode" style="display:none"></div>
       </div>
       <div class="card">
         <strong>Mods con acceso</strong>
         <p class="sub" style="margin:6px 0 12px">Lista real de mods de tu canal, traída de Twitch. Marcá quiénes pueden entrar a <code>/mod</code>.</p>
         <div id="mods"></div>
       </div>
       <div class="card">
         <strong>Política de envío</strong>
         <label class="sw" style="margin-top:12px"><input type="checkbox" id="submissions_open"><span class="name">Envíos abiertos</span></label>
         <p class="sub" style="margin:6px 0 0">Normalmente lo maneja Twitch solo: se abren al arrancar el stream y se cierran un rato después de terminarlo. Tocá esto para probar sin estar en vivo, o para abrirlos a mano si Twitch no avisó.</p>
         <div class="grid" style="margin-top:12px">
           <div><label>Cooldown por usuario (s)</label><input type="number" id="cooldown_seconds"></div>
           <div><label>Máximo en cola por usuario</label><input type="number" id="max_pending_per_user"></div>
           <div><label>Duración máxima (s)</label><input type="number" id="max_duration_seconds"></div>
           <div><label>Tamaño máximo (MB)</label><input type="number" id="max_filesize_mb"></div>
           <div><label>Resolución máxima</label><select id="max_resolution"><option value="720">720</option><option value="1080">1080</option></select></div>
           <div><label>Gap entre videos (s)</label><input type="number" id="playback_gap_seconds"></div>
         </div>
         <div class="row" style="margin-top:14px"><button id="saveBtn">Guardar</button></div>
         <div class="msg" id="msg"></div>
       </div>
     </div>`,
    `
(async function () {
  let me;
  try { me = await api('/api/me'); } catch (e) { $('#chLine').textContent = e.message; return; }

  if (!me.login) { $('#authCard').style.display = ''; $('#loginBtn').onclick = () => login('/admin'); return; }
  $('#chLine').textContent = 'Conectado como ' + me.login;

  let data;
  try { data = await api('/api/admin/overview'); }
  catch (e) {
    $('#authCard').style.display = '';
    $('#authMsg').textContent = e.message;
    $('#loginBtn').textContent = 'Entrar con otra cuenta';
    $('#loginBtn').onclick = () => login('/admin');
    return;
  }
  $('#panel').style.display = '';

  const box = $('#mods'); box.textContent = '';
  if (!data.mods.length) box.appendChild(el('div', 'empty', 'Tu canal no tiene mods en Twitch.'));
  for (const m of data.mods) {
    const row = el('div', 'mod-row');
    const lab = el('label', 'sw');
    const cb = el('input'); cb.type = 'checkbox'; cb.checked = m.authorized;
    cb.onchange = async () => {
      cb.disabled = true;
      try {
        await api('/api/admin/mods', { method: 'POST', headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ user_id: m.user_id, login: m.user_login, authorized: cb.checked }) });
      } catch (e) { alert(e.message); cb.checked = !cb.checked; }
      cb.disabled = false;
    };
    lab.appendChild(cb);
    lab.appendChild(el('span', 'name', m.user_name || m.user_login));
    row.appendChild(lab);
    if (!m.still_mod) row.appendChild(el('span', 'pill warn', 'ya no es mod en Twitch'));
    box.appendChild(row);
  }

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
      const box = $('#pairCode');
      box.style.display = '';
      box.textContent = r.code;
    } catch (e) { alert(e.message); }
    this.disabled = false;
  };
})();`,
  );
}

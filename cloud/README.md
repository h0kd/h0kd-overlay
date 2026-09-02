# cloud/ — Worker + Durable Object + D1

La mitad en la nube de **Video Requests**: sirve `/submit`, `/mod` y `/admin`,
resuelve el OAuth de Twitch, guarda la cola en D1 y coordina en vivo con el
agente local (la app de escritorio) a través de un Durable Object por canal.

El contrato de mensajes con el agente está en [`../docs/ws-protocol.md`](../docs/ws-protocol.md).

## Cómo está armado

```
src/
  index.ts     Worker: páginas, API, OAuth, /agent/pair, /eventsub
  do.ts        ChannelHub: un Durable Object por canal (hub de WebSocket)
  auth.ts      sesiones firmadas, state anti-CSRF, resolución de rol
  twitch.ts    OAuth Authorization Code Flow + Helix + suscripciones EventSub
  eventsub.ts  verificación de firma del webhook
  policy.ts    allowlist de dominios y política de envío
  queue.ts     acceso a D1
  crypto.ts    HMAC, AES-GCM, comparación en tiempo constante
  pages.ts     el HTML de las tres páginas
  protocol.ts  espejo TS del contrato con el agente
schema.sql     tablas de D1
test/          tests de la lógica pura (corren sin Cloudflare)
```

## Puesta en marcha

### 1. App de Twitch

En <https://dev.twitch.tv/console/apps>, creá una app **confidencial**:

- **OAuth Redirect URL:** `https://TU-WORKER.workers.dev/auth/callback`
- Guardá el **Client ID** y generá un **Client Secret**.

> Es una app distinta de la que usa el agente. El agente usa Device Code Flow
> con un Client ID público; acá hay un secret, y vive solo en el Worker.

### 2. Base de datos

```bash
npm install
cp wrangler.example.toml wrangler.toml     # la config real no se versiona
npx wrangler d1 create video-requests      # copiá el database_id a wrangler.toml
npm run db:remote                          # aplica schema.sql
```

> `wrangler.toml` está en .gitignore: lleva el id de tu base D1, el Client ID de
> tu app de Twitch y el subdominio de tu cuenta. Nada de eso es un secret, pero
> tampoco hace falta publicarlo.

### 3. Variables y secrets

En `wrangler.toml` completá `TWITCH_CLIENT_ID`, `PUBLIC_ORIGIN` y
`ALLOWED_CHANNELS` (los logins del beta, separados por coma). Para sumar un
canal de punta a punta, ver `docs/alta-de-canal.md`.

Los secrets **no van al repo**:

```bash
npx wrangler secret put TWITCH_CLIENT_SECRET
npx wrangler secret put SESSION_SECRET      # ver abajo cómo generarlos
npx wrangler secret put TOKEN_ENC_KEY
npx wrangler secret put EVENTSUB_SECRET
```

Generar los tres valores aleatorios:

```bash
node -e "console.log(require('crypto').randomBytes(32).toString('base64'))"
```

`TOKEN_ENC_KEY` tiene que ser exactamente de **32 bytes en base64** (la línea de
arriba ya da eso). Los otros dos pueden ser cualquier cadena larga y aleatoria.

### 4. Desplegar

```bash
npm run deploy
```

Después entrá a `https://TU-WORKER.workers.dev/admin` con la cuenta del canal
(te deja en `/admin?ch=tu-canal`; ese es el link para volver).
Ese primer login hace tres cosas: da de alta el canal, guarda el token para
consultar Helix, y suscribe `stream.online` / `stream.offline` al webhook.

## Desarrollo local

```bash
npm test          # tests de lógica pura, sin Cloudflare ni red
npm run typecheck
npm run db:local  # aplica el schema a la D1 local
npm run dev       # wrangler dev
```

El OAuth de Twitch no funciona contra `localhost` salvo que registres
`http://localhost:8787/auth/callback` como redirect adicional en la app de
Twitch y pongas `PUBLIC_ORIGIN=http://localhost:8787`.

## Decisiones que conviene conocer antes de tocar el código

**La identidad nunca viene del cliente.** Ni el `channel_id`, ni el user id, ni
el rol. Todo sale de la cookie de sesión firmada; el formulario de envío solo
lleva el link. Si algún endpoint nuevo necesita saber "quién", la respuesta es
`getSession()`, nunca el body.

**La allowlist corre antes que yt-dlp, siempre.** Sin ella, yt-dlp cae en su
extractor genérico y la feature se convierte en "traeme esta URL" a discreción
de cualquiera que escriba en el chat. El agente igual revalida: la allowlist del
Worker es la primera defensa, no la única.

**El agente reporta hechos; la nube deriva estados.** El agente nunca dice "esto
quedó en pending_review", dice "esto dura 14 segundos". Por eso, si el agente se
cae en cualquier punto, el estado verdadero sigue completo en D1.

**Lo rechazado no se descarga nunca.** Un `download.request` sale únicamente
después de que un humano aprobó. Es la razón de ser de la feature de moderación.

**El fin de stream espera.** `stream.offline` no vacía la cola: arma una alarma
en el DO con ~45 s de gracia. Una microcaída de internet manda offline y vuelve
enseguida, y vaciarle la cola al streamer por eso es peor que esperar.

**El DO usa la Hibernation API.** Los sockets sobreviven a que el objeto sea
evacuado de memoria, que es lo normal en un stream de horas. Consecuencia
práctica: no guardes estado en campos de instancia esperando que siga ahí —
lo que tiene que durar va a `ctx.storage`, y lo que distingue un socket de otro
va en sus tags.

**Los tokens del broadcaster se guardan cifrados** (AES-GCM con `TOKEN_ENC_KEY`)
y los del agente, hasheados. Si alguien lee la base, no se lleva credenciales
utilizables.

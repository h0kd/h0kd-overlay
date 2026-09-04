# Dar de alta un canal en Video Requests (procedimiento manual)

Cómo sumamos un streamer nuevo al Worker. El alta es **manual a propósito**:
la lista de canales vive en la config del Worker y la tocamos nosotros, así
controlamos quién entra mientras el módulo es beta. La guía que sigue el
streamer está en [`video-requests.md`](video-requests.md); esto es la parte
que hace quien administra el Worker.

Cada canal es independiente: cola, mods, historial y ajustes propios.

## 1. Habilitar el canal (quien administra el Worker)

1. En `cloud/wrangler.toml` (no versionado), agregar el login de Twitch a la
   lista, separado por coma:
   ```toml
   ALLOWED_CHANNELS = "h0kd,iza42,nuevo_canal"
   ```
   Sin ese login, al entrar a `/admin` la página dice que el canal "todavía
   no está habilitado en el beta".
2. Desplegar:
   ```bash
   cd cloud && npx wrangler deploy
   ```
3. Avisarle al streamer que ya puede entrar a `https://videos.h0kd.dev/admin`
   con su cuenta de Twitch, y pasarle el link a la guía.

Con más de un canal, `/submit` y `/mod` sin `?ch=` no redirigen solos: cada
canal comparte sus links con `?ch=<login>` (están en su `/admin`).

## 2. Lo que hace el streamer

Está en [`video-requests.md`](video-requests.md), resumido:

1. Entra a `/admin` con su cuenta de Twitch (registra el canal, guarda el
   token de Helix y crea las suscripciones de EventSub).
2. Marca sus mods en **Moderadores** y revisa la política en **Ajustes**.
3. En la app: **Activar módulo**, Guardar, reiniciar, **Instalar lo que
   falte**, **Emparejar** con el código de `/admin` (pestaña Ajustes).
4. OBS: el mismo Browser Source de siempre, `http://localhost:3001/overlay`.

Si algo del lado del streamer no anda, lo primero es su log del día en
`%APPDATA%\Stream Overlay\logs\`.

## Para probar sin estar en vivo

En `/admin`, pestaña Ajustes, la casilla **Envíos abiertos**. El próximo
evento de stream (online/offline) vuelve a mandar.

## Dar de baja un canal

Sacar el login de `ALLOWED_CHANNELS` y desplegar. Sus páginas dejan de
responder; los datos en D1 quedan hasta que se borren a mano.

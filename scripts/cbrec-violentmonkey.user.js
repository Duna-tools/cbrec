// ==UserScript==
// @name         cbrec browser session bridge
// @namespace    https://github.com/Duna-tools/cbrec
// @version      1.0.0
// @description  Sends the active Chaturbate session to one cbrec command.
// @match        https://chaturbate.com/*
// @match        https://www.chaturbate.com/*
// @grant        GM_cookie
// @grant        GM_xmlhttpRequest
// @grant        GM_registerMenuCommand
// @connect      127.0.0.1
// ==/UserScript==

(() => {
  "use strict";

  const SESSION_COOKIE_NAMES = ["PHPSESSID", "chaturbatesid"];
  const SESSION_PATH_PREFIX = "/cbrec-session/";

  function listCookies() {
    return new Promise((resolve, reject) => {
      GM_cookie.list({ url: window.location.origin }, (cookies, error) => {
        if (error) {
          reject(new Error(error));
          return;
        }
        resolve(cookies || []);
      });
    });
  }

  function sessionCookie(cookies) {
    const values = new Map(cookies.map((cookie) => [cookie.name, cookie.value]));
    return SESSION_COOKIE_NAMES
      .filter((name) => values.has(name))
      .map((name) => `${name}=${values.get(name)}`)
      .join("; ");
  }

  function receiveUrl() {
    const value = window.prompt("Pega la dirección que muestra cbrec:");
    if (!value) {
      return null;
    }

    let url;
    try {
      url = new URL(value);
    } catch {
      window.alert("La dirección no es válida.");
      return null;
    }

    if (
      url.protocol !== "http:" ||
      url.hostname !== "127.0.0.1" ||
      !url.port ||
      !url.pathname.startsWith(SESSION_PATH_PREFIX)
    ) {
      window.alert("La dirección debe ser el receptor local mostrado por cbrec.");
      return null;
    }
    return url.href;
  }

  function sendSession(url, cookie) {
    GM_xmlhttpRequest({
      method: "POST",
      url,
      headers: { "Content-Type": "application/json" },
      data: JSON.stringify({ session_cookie: cookie }),
      anonymous: true,
      timeout: 10_000,
      onload: (response) => {
        window.alert(
          response.status === 204
            ? "Sesión enviada a cbrec."
            : "cbrec rechazó la sesión; verifica la dirección y vuelve a intentarlo.",
        );
      },
      onerror: () => window.alert("No se pudo conectar con cbrec."),
      ontimeout: () => window.alert("La conexión con cbrec agotó el tiempo."),
    });
  }

  GM_registerMenuCommand("Enviar sesión a cbrec", async () => {
    const url = receiveUrl();
    if (!url) {
      return;
    }

    try {
      const cookie = sessionCookie(await listCookies());
      if (!cookie) {
        window.alert("No se encontró una cookie de sesión de Chaturbate.");
        return;
      }
      sendSession(url, cookie);
    } catch {
      window.alert("Violentmonkey no pudo leer las cookies. Revisa el permiso HTTP-only.");
    }
  });
})();

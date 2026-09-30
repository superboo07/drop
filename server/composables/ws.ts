import type { NuxtError } from "#app";

export type WebSocketCallback = (message: string) => void;
export type WebSocketErrorHandler = (error: NuxtError<unknown>) => void;
export type WebSocketOpenCallback = () => void;

// Reconnect backoff: 1s, 2s, 4s... capped at 30s, reset once a socket opens.
const RECONNECT_BASE_MS = 1000;
const RECONNECT_MAX_MS = 30_000;

/**
 * A websocket that reconnects on its own. Sockets get closed from under us
 * (nginx drops idle ones after a minute, the server restarts), and a page
 * that keeps sending into a closed one never hears back.
 *
 * Nothing sent before a close is replayed by the server, so anything that
 * subscribes to something should re-subscribe in an `onOpen` callback.
 */
export class WebSocketHandler {
  private listeners: Array<WebSocketCallback> = [];
  private openListeners: Array<WebSocketOpenCallback> = [];

  private outQueue: Array<string> = [];
  private inQueue: Array<string> = [];

  private ws: WebSocket | undefined = undefined;
  private url: string | undefined = undefined;
  private reconnectAttempts = 0;

  private errorHandler: WebSocketErrorHandler | undefined = undefined;

  constructor(route: string) {
    if (import.meta.server) return;
    const isSecure = location.protocol === "https:";
    this.url = (isSecure ? "wss://" : "ws://") + location.host + route;
    this.open();
  }

  get connected() {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  private open() {
    if (!this.url) return;
    const ws = new WebSocket(this.url);
    this.ws = ws;

    ws.onopen = () => {
      this.reconnectAttempts = 0;
      for (const callback of this.openListeners) {
        callback();
      }
      const queue = this.outQueue;
      this.outQueue = [];
      for (const message of queue) {
        ws.send(message);
      }
    };

    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = undefined;
      const delay = Math.min(
        RECONNECT_BASE_MS * 2 ** this.reconnectAttempts,
        RECONNECT_MAX_MS,
      );
      this.reconnectAttempts++;
      setTimeout(() => this.open(), delay);
    };

    ws.onmessage = (e) => {
      const message = e.data;
      switch (message) {
        case "unauthenticated": {
          const error = createError({
            statusCode: 403,
            statusMessage: "Unable to connect to websocket - unauthenticated",
          });
          if (this.errorHandler) {
            return this.errorHandler(error);
          } else {
            throw error;
          }
        }
      }
      if (this.listeners.length == 0) {
        this.inQueue.push(message);
        return;
      }

      for (const listener of this.listeners) {
        listener(message);
      }
    };
  }

  error(handler: WebSocketErrorHandler) {
    this.errorHandler = handler;
  }

  listen(callback: WebSocketCallback) {
    this.listeners.push(callback);
    const queue = this.inQueue;
    this.inQueue = [];
    for (const message of queue) {
      callback(message);
    }
  }

  /**
   * Called every time the socket opens, the first time and after each
   * reconnect, before any queued messages are sent.
   */
  onOpen(callback: WebSocketOpenCallback) {
    this.openListeners.push(callback);
  }

  send(message: string) {
    if (!this.connected || !this.ws) {
      this.outQueue.push(message);
      return;
    }

    this.ws.send(message);
  }
}

import { vi } from "vitest";

type Handler = (args: Record<string, unknown> | undefined) => unknown;
type Listener = (event: { payload: unknown }) => void;

interface MockState {
  handlers: Map<string, Handler>;
  listeners: Map<string, Set<Listener>>;
  invoke: ReturnType<typeof createInvoke>;
  listen: ReturnType<typeof createListen>;
}

function createInvoke() {
  return vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    const handler = state.handlers.get(cmd);
    if (!handler) throw new Error(`unmocked command: ${cmd}`);
    return handler(args);
  });
}

function createListen() {
  return vi.fn(async (event: string, listener: Listener) => {
    let set = state.listeners.get(event);
    if (!set) state.listeners.set(event, (set = new Set()));
    set.add(listener);
    return () => {
      state.listeners.get(event)?.delete(listener);
    };
  });
}

// Kept on globalThis so modules re-imported after vi.resetModules() share one registry and one set of spies.
const g = globalThis as typeof globalThis & { __kyraTauriMock?: MockState };
const state: MockState = (g.__kyraTauriMock ??= {
  handlers: new Map(),
  listeners: new Map(),
  invoke: createInvoke(),
  listen: createListen(),
});

export const invokeMock = state.invoke;
export const listenMock = state.listen;

export function onInvoke(cmd: string, handler: Handler) {
  state.handlers.set(cmd, handler);
}

export function emit(event: string, payload: unknown) {
  for (const listener of [...(state.listeners.get(event) ?? [])]) listener({ payload });
}

export function listenerCount(event: string): number {
  return state.listeners.get(event)?.size ?? 0;
}

export function invokedWith(cmd: string): Record<string, unknown>[] {
  return invokeMock.mock.calls.filter(([c]) => c === cmd).map(([, args]) => args ?? {});
}

export function resetTauri() {
  state.handlers.clear();
  state.listeners.clear();
  invokeMock.mockClear();
  listenMock.mockClear();
}

/** Resolves pending promise chains without advancing fake timers. */
export async function flush() {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

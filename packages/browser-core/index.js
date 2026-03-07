import initWasm, {
  abi_fingerprint as rawAbiFingerprint,
  abi_version as rawAbiVersion,
  fetch_request as rawFetchRequest,
  runtime_close as rawRuntimeClose,
  runtime_create as rawRuntimeCreate,
  scope_close as rawScopeClose,
  scope_enter as rawScopeEnter,
  task_cancel as rawTaskCancel,
  task_join as rawTaskJoin,
  task_spawn as rawTaskSpawn,
  websocket_cancel as rawWebSocketCancel,
  websocket_close as rawWebSocketClose,
  websocket_open as rawWebSocketOpen,
  websocket_recv as rawWebSocketRecv,
  websocket_send as rawWebSocketSend,
} from "./asupersync.js";

const HANDLE_KINDS = new Set([
  "runtime",
  "region",
  "task",
  "cancel_token",
  "fetch_request",
]);

const CANCELLATION_PHASE_ORDER = Object.freeze([
  "requested",
  "draining",
  "finalizing",
  "completed",
]);

const ERROR_CODES = Object.freeze([
  "capability_denied",
  "invalid_handle",
  "decode_failure",
  "compatibility_rejected",
  "internal_failure",
]);

const RECOVERABILITY_LEVELS = Object.freeze([
  "transient",
  "permanent",
  "unknown",
]);

const BUDGET_BOUNDS = Object.freeze({
  pollQuota: Object.freeze({ min: 1, max: 1_000_000 }),
  deadlineMs: Object.freeze({ min: 0, max: 86_400_000 }),
  priority: Object.freeze({ min: 0, max: 255 }),
  cleanupQuota: Object.freeze({ min: 0, max: 1_000_000 }),
});

function parseJson(raw, label) {
  if (typeof raw !== "string") {
    throw new TypeError(`${label} must be a JSON string`);
  }
  try {
    return JSON.parse(raw);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`${label} returned invalid JSON: ${message}`);
  }
}

function consumerVersionJson(consumerVersion) {
  return consumerVersion === null || consumerVersion === undefined
    ? undefined
    : JSON.stringify(consumerVersion);
}

function errorMessage(error) {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  return String(error);
}

function normalizeFailure(error, label) {
  const message = errorMessage(error);
  try {
    const failure = JSON.parse(message);
    if (
      failure &&
      typeof failure === "object" &&
      typeof failure.code === "string" &&
      typeof failure.recoverability === "string" &&
      typeof failure.message === "string"
    ) {
      return failure;
    }
  } catch {
    // Fall through to the generic failure shape below.
  }
  return {
    code: "internal_failure",
    recoverability: "unknown",
    message: `${label} failed: ${message}`,
  };
}

function normalizeByteArray(bytes, label) {
  if (bytes instanceof Uint8Array) {
    return Array.from(bytes);
  }
  if (bytes instanceof ArrayBuffer) {
    return Array.from(new Uint8Array(bytes));
  }
  if (ArrayBuffer.isView(bytes)) {
    return Array.from(new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength));
  }
  if (
    Array.isArray(bytes) &&
    bytes.every((value) => Number.isInteger(value) && value >= 0 && value <= 255)
  ) {
    return [...bytes];
  }
  throw new TypeError(`${label} must be Uint8Array, ArrayBuffer, ArrayBufferView, or byte[]`);
}

function normalizeBudgetNumber(name, value) {
  if (!Number.isInteger(value)) {
    throw new TypeError(`Budget.${name} must be an integer`);
  }
  const bounds = BUDGET_BOUNDS[name];
  if (value < bounds.min || value > bounds.max) {
    throw new RangeError(
      `Budget.${name} must be between ${bounds.min} and ${bounds.max}; received ${value}`,
    );
  }
  return value;
}

export function createBudget(input = {}) {
  return {
    pollQuota: normalizeBudgetNumber("pollQuota", input.pollQuota ?? 1_024),
    deadlineMs: normalizeBudgetNumber("deadlineMs", input.deadlineMs ?? 30_000),
    priority: normalizeBudgetNumber("priority", input.priority ?? 100),
    cleanupQuota: normalizeBudgetNumber("cleanupQuota", input.cleanupQuota ?? 256),
  };
}

function isRawHandle(value) {
  return (
    Boolean(value) &&
    typeof value === "object" &&
    typeof value.kind === "string" &&
    HANDLE_KINDS.has(value.kind) &&
    Number.isInteger(value.slot) &&
    Number.isInteger(value.generation)
  );
}

function normalizeHandle(handle, label, expectedKind) {
  const raw = handle instanceof BaseHandle ? handle.toJSON() : handle;
  if (!isRawHandle(raw)) {
    throw new TypeError(`${label} must be a browser-core handle`);
  }
  if (expectedKind && raw.kind !== expectedKind) {
    throw new TypeError(`${label} must be a ${expectedKind} handle; received ${raw.kind}`);
  }
  return {
    kind: raw.kind,
    slot: raw.slot,
    generation: raw.generation,
  };
}

function wrapHandle(rawHandle) {
  const handle = normalizeHandle(rawHandle, "value");
  switch (handle.kind) {
    case "runtime":
      return new RuntimeHandle(handle);
    case "region":
      return new RegionHandle(handle);
    case "task":
      return new TaskHandle(handle);
    case "cancel_token":
      return new CancellationToken(handle);
    case "fetch_request":
      return new FetchHandle(handle);
    default:
      throw new TypeError(`Unsupported handle kind ${handle.kind}`);
  }
}

function parseHandleResult(rawHandle, label, expectedKind) {
  return wrapHandle(normalizeHandle(parseJson(rawHandle, label), label, expectedKind));
}

function reviveValue(rawValue) {
  if (!rawValue || typeof rawValue !== "object" || typeof rawValue.kind !== "string") {
    throw new TypeError("Outcome value must use the WASM ABI tagged-value shape");
  }
  switch (rawValue.kind) {
    case "unit":
      return undefined;
    case "bool":
    case "i64":
    case "u64":
    case "string":
      return rawValue.value;
    case "bytes":
      return Uint8Array.from(rawValue.value ?? []);
    case "handle":
      return wrapHandle(rawValue.value);
    default:
      throw new TypeError(`Unsupported ABI value kind ${rawValue.kind}`);
  }
}

function encodeValue(value, label) {
  if (value === undefined) {
    return { kind: "unit" };
  }
  if (typeof value === "boolean") {
    return { kind: "bool", value };
  }
  if (typeof value === "number") {
    if (!Number.isFinite(value) || !Number.isInteger(value)) {
      throw new TypeError(`${label} must be a finite integer`);
    }
    return value >= 0 ? { kind: "u64", value } : { kind: "i64", value };
  }
  if (typeof value === "string") {
    return { kind: "string", value };
  }
  if (
    value instanceof Uint8Array ||
    value instanceof ArrayBuffer ||
    ArrayBuffer.isView(value) ||
    Array.isArray(value)
  ) {
    return { kind: "bytes", value: normalizeByteArray(value, label) };
  }
  if (value instanceof BaseHandle || isRawHandle(value)) {
    return { kind: "handle", value: normalizeHandle(value, label) };
  }
  if (value && typeof value === "object" && typeof value.kind === "string") {
    return value;
  }
  throw new TypeError(`${label} is not encodable across the WASM ABI boundary`);
}

function reviveOutcomeEnvelope(rawOutcome, label) {
  const outcome = parseJson(rawOutcome, label);
  if (!outcome || typeof outcome !== "object" || typeof outcome.outcome !== "string") {
    throw new TypeError(`${label} must decode to a tagged outcome envelope`);
  }
  if (outcome.outcome === "ok") {
    return {
      outcome: "ok",
      value: reviveValue(outcome.value),
    };
  }
  return outcome;
}

function encodeOutcomeEnvelope(outcome, label) {
  if (!outcome || typeof outcome !== "object" || typeof outcome.outcome !== "string") {
    throw new TypeError(`${label} must be a tagged outcome envelope`);
  }
  if (outcome.outcome === "ok") {
    return {
      outcome: "ok",
      value: encodeValue(outcome.value, `${label}.value`),
    };
  }
  return outcome;
}

function invokeHandleOperation(label, expectedKind, fn) {
  try {
    return Outcome.ok(parseHandleResult(fn(), `${label}.response`, expectedKind));
  } catch (error) {
    return {
      outcome: "err",
      failure: normalizeFailure(error, label),
    };
  }
}

function invokeOutcomeOperation(label, fn) {
  try {
    return reviveOutcomeEnvelope(fn(), `${label}.response`);
  } catch (error) {
    return {
      outcome: "err",
      failure: normalizeFailure(error, label),
    };
  }
}

export const Outcome = Object.freeze({
  ok(value) {
    return { outcome: "ok", value };
  },
  err(code, recoverability, message) {
    return {
      outcome: "err",
      failure: { code, recoverability, message },
    };
  },
  cancelled(cancellation) {
    return { outcome: "cancelled", cancellation };
  },
  panicked(message) {
    return { outcome: "panicked", message };
  },
});

export class BaseHandle {
  constructor(rawHandle, expectedKind) {
    const handle = normalizeHandle(rawHandle, "handle", expectedKind);
    this.kind = handle.kind;
    this.slot = handle.slot;
    this.generation = handle.generation;
    Object.freeze(this);
  }

  toJSON() {
    return {
      kind: this.kind,
      slot: this.slot,
      generation: this.generation,
    };
  }
}

export class RuntimeHandle extends BaseHandle {
  constructor(rawHandle) {
    super(rawHandle, "runtime");
  }

  close(consumerVersion = null) {
    return runtime_close(this, consumerVersion);
  }

  enterScope(label = undefined, consumerVersion = null) {
    return scope_enter({ parent: this, label }, consumerVersion);
  }
}

export class RegionHandle extends BaseHandle {
  constructor(rawHandle) {
    super(rawHandle, "region");
  }

  close(consumerVersion = null) {
    return scope_close(this, consumerVersion);
  }

  enterScope(label = undefined, consumerVersion = null) {
    return scope_enter({ parent: this, label }, consumerVersion);
  }

  spawnTask(options = {}, consumerVersion = null) {
    return task_spawn({ scope: this, ...options }, consumerVersion);
  }

  fetchRequest(options, consumerVersion = null) {
    return fetch_request({ scope: this, ...options }, consumerVersion);
  }

  openWebSocket(url, protocols = undefined, consumerVersion = null) {
    return websocket_open({ scope: this, url, protocols }, consumerVersion);
  }
}

export class TaskHandle extends BaseHandle {
  constructor(rawHandle) {
    super(rawHandle, "task");
  }

  join(outcome, consumerVersion = null) {
    return task_join(this, outcome, consumerVersion);
  }

  cancel(kind, message = undefined, consumerVersion = null) {
    return task_cancel({ task: this, kind, message }, consumerVersion);
  }
}

export class CancellationToken extends BaseHandle {
  constructor(rawHandle) {
    super(rawHandle, "cancel_token");
  }
}

export class FetchHandle extends BaseHandle {
  constructor(rawHandle) {
    super(rawHandle, "fetch_request");
  }
}

async function init(input) {
  return initWasm(input);
}

export default init;
export { init };

export function runtime_create(consumerVersion = null) {
  return invokeHandleOperation("runtime_create", "runtime", () =>
    rawRuntimeCreate(consumerVersionJson(consumerVersion)),
  );
}

export function runtime_close(runtimeHandle, consumerVersion = null) {
  return invokeOutcomeOperation("runtime_close", () =>
    rawRuntimeClose(
      JSON.stringify(normalizeHandle(runtimeHandle, "runtimeHandle", "runtime")),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function scope_enter(request, consumerVersion = null) {
  return invokeHandleOperation("scope_enter", "region", () =>
    rawScopeEnter(
      JSON.stringify({
        parent: normalizeHandle(request.parent, "request.parent"),
        label: request.label ?? undefined,
      }),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function scope_close(regionHandle, consumerVersion = null) {
  return invokeOutcomeOperation("scope_close", () =>
    rawScopeClose(
      JSON.stringify(normalizeHandle(regionHandle, "regionHandle", "region")),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function task_spawn(request, consumerVersion = null) {
  return invokeHandleOperation("task_spawn", "task", () =>
    rawTaskSpawn(
      JSON.stringify({
        scope: normalizeHandle(request.scope, "request.scope", "region"),
        label: request.label ?? undefined,
        cancel_kind: request.cancel_kind ?? undefined,
      }),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function task_join(taskHandle, outcome, consumerVersion = null) {
  return invokeOutcomeOperation("task_join", () =>
    rawTaskJoin(
      JSON.stringify(normalizeHandle(taskHandle, "taskHandle", "task")),
      JSON.stringify(encodeOutcomeEnvelope(outcome, "outcome")),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function task_cancel(request, consumerVersion = null) {
  return invokeOutcomeOperation("task_cancel", () =>
    rawTaskCancel(
      JSON.stringify({
        task: normalizeHandle(request.task, "request.task", "task"),
        kind: request.kind,
        message: request.message ?? undefined,
      }),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function fetch_request(request, consumerVersion = null) {
  return invokeOutcomeOperation("fetch_request", () =>
    rawFetchRequest(
      JSON.stringify({
        scope: normalizeHandle(request.scope, "request.scope", "region"),
        url: request.url,
        method: request.method,
        body:
          request.body === null || request.body === undefined
            ? undefined
            : normalizeByteArray(request.body, "request.body"),
      }),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function websocket_open(request, consumerVersion = null) {
  return invokeOutcomeOperation("websocket_open", () =>
    rawWebSocketOpen(
      JSON.stringify({
        scope: normalizeHandle(request.scope, "request.scope", "region"),
        url: request.url,
        protocols: request.protocols ?? undefined,
      }),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function websocket_send(request, consumerVersion = null) {
  return invokeOutcomeOperation("websocket_send", () =>
    rawWebSocketSend(
      JSON.stringify({
        socket: normalizeHandle(request.socket, "request.socket", "task"),
        value: encodeValue(request.value, "request.value"),
      }),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function websocket_recv(request, consumerVersion = null) {
  return invokeOutcomeOperation("websocket_recv", () =>
    rawWebSocketRecv(
      JSON.stringify({
        socket: normalizeHandle(request.socket, "request.socket", "task"),
      }),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function websocket_close(request, consumerVersion = null) {
  return invokeOutcomeOperation("websocket_close", () =>
    rawWebSocketClose(
      JSON.stringify({
        socket: normalizeHandle(request.socket, "request.socket", "task"),
        reason: request.reason ?? undefined,
      }),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function websocket_cancel(request, consumerVersion = null) {
  return invokeOutcomeOperation("websocket_cancel", () =>
    rawWebSocketCancel(
      JSON.stringify({
        socket: normalizeHandle(request.socket, "request.socket", "task"),
        kind: request.kind,
        message: request.message ?? undefined,
      }),
      consumerVersionJson(consumerVersion),
    ),
  );
}

export function abi_version() {
  return parseJson(rawAbiVersion(), "abi_version");
}

export function abi_fingerprint() {
  return rawAbiFingerprint();
}

export const runtimeCreate = runtime_create;
export const runtimeClose = runtime_close;
export const scopeEnter = scope_enter;
export const scopeClose = scope_close;
export const taskSpawn = task_spawn;
export const taskJoin = task_join;
export const taskCancel = task_cancel;
export const fetchRequest = fetch_request;
export const websocketOpen = websocket_open;
export const websocketSend = websocket_send;
export const websocketRecv = websocket_recv;
export const websocketClose = websocket_close;
export const websocketCancel = websocket_cancel;
export const abiVersion = abi_version;
export const abiFingerprint = abi_fingerprint;

export const rawBindings = Object.freeze({
  init: initWasm,
  runtime_create: rawRuntimeCreate,
  runtime_close: rawRuntimeClose,
  scope_enter: rawScopeEnter,
  scope_close: rawScopeClose,
  task_spawn: rawTaskSpawn,
  task_join: rawTaskJoin,
  task_cancel: rawTaskCancel,
  fetch_request: rawFetchRequest,
  websocket_open: rawWebSocketOpen,
  websocket_send: rawWebSocketSend,
  websocket_recv: rawWebSocketRecv,
  websocket_close: rawWebSocketClose,
  websocket_cancel: rawWebSocketCancel,
  abi_version: rawAbiVersion,
  abi_fingerprint: rawAbiFingerprint,
});

export {
  BUDGET_BOUNDS,
  CANCELLATION_PHASE_ORDER,
  ERROR_CODES,
  RECOVERABILITY_LEVELS,
};

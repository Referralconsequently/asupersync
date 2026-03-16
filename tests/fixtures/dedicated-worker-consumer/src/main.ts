type WorkerBootstrapMessage = {
  type: "worker-bootstrap";
  payload: unknown;
};

type WorkerBootstrapFailedMessage = {
  type: "worker-bootstrap-failed";
  message: string;
};

type WorkerShutdownMessage = {
  type: "worker-shutdown-complete";
  reason: string | null;
};

type WorkerMessage =
  | WorkerBootstrapMessage
  | WorkerBootstrapFailedMessage
  | WorkerShutdownMessage;

const statusElement = document.getElementById("status");
if (!statusElement) {
  throw new Error("status element missing");
}

const worker = new Worker(new URL("./worker.ts", import.meta.url), {
  type: "module",
});

const state = {
  phase: "spawning",
  events: [] as WorkerMessage[],
};

const render = () => {
  statusElement.textContent = JSON.stringify(state, null, 2);
};

worker.addEventListener("message", (event: MessageEvent<WorkerMessage>) => {
  state.events.push(event.data);

  if (event.data.type === "worker-bootstrap") {
    state.phase = "worker_ready";
    render();
    worker.postMessage({
      type: "shutdown",
      reason: "fixture-handoff-complete",
    });
    return;
  }

  if (event.data.type === "worker-shutdown-complete") {
    state.phase = "shutdown_complete";
    render();
    worker.terminate();
    return;
  }

  state.phase = "worker_error";
  render();
});

worker.addEventListener("error", (event) => {
  state.phase = "worker_error";
  state.events.push({
    type: "worker-bootstrap-failed",
    message: event.message || "worker bootstrap failed",
  });
  render();
});

render();

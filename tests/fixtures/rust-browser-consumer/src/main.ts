import init, {
  run_rust_browser_consumer_demo,
} from "../pkg/asupersync_rust_browser_consumer_fixture.js";

const statusElement = document.getElementById("status");
if (!statusElement) {
  throw new Error("status element missing");
}

const render = (value: unknown): void => {
  statusElement.textContent = JSON.stringify(value, null, 2);
};

async function main(): Promise<void> {
  render({
    phase: "initializing",
    scenario: "rust-browser-consumer",
  });

  await init();
  const summary = run_rust_browser_consumer_demo();
  render(summary);
}

void main().catch((error) => {
  render({
    phase: "error",
    message:
      error instanceof Error ? error.message : typeof error === "string" ? error : "unknown error",
  });
});

import { detectNextRuntimeSupport } from "@asupersync/next";

export const runtime = "edge";

export async function GET() {
  return Response.json({
    target: "edge",
    bridgeOnly: true,
    diagnostics: detectNextRuntimeSupport("edge"),
    note:
      "Edge routes stay bridge-only in this example; direct Browser Edition runtime creation remains a client concern.",
  });
}

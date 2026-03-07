/**
 * @asupersync/browser — High-level Browser Edition SDK surface.
 *
 * Re-exports the low-level runtime bindings from @asupersync/browser-core
 * and adds SDK-level ergonomics (init helpers, diagnostics, lifecycle).
 */

export { default as init } from "@asupersync/browser-core";
export * from "@asupersync/browser-core";

/** ABI metadata re-exported for diagnostics. */
export { default as abiMetadata } from "@asupersync/browser-core/abi-metadata.json";

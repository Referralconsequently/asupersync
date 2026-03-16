import fs from "node:fs";
import path from "node:path";

const distDir = path.resolve("dist");
const indexPath = path.join(distDir, "index.html");
const assetDir = path.join(distDir, "assets");

if (!fs.existsSync(distDir)) {
  throw new Error(`Missing dist directory: ${distDir}`);
}

if (!fs.existsSync(indexPath)) {
  throw new Error(`Missing built index.html: ${indexPath}`);
}

if (!fs.existsSync(assetDir)) {
  throw new Error(`Missing assets directory: ${assetDir}`);
}

const assets = fs.readdirSync(assetDir);
const jsAssets = assets.filter((name) => name.endsWith(".js") || name.endsWith(".mjs"));
const wasmAssets = assets.filter((name) => name.endsWith(".wasm"));

if (jsAssets.length === 0) {
  throw new Error("Expected at least one JavaScript asset in dist/assets");
}

if (wasmAssets.length === 0) {
  throw new Error("Expected at least one wasm asset in dist/assets");
}

const indexHtml = fs.readFileSync(indexPath, "utf8");
if (!/(?:^|["'(])(?:\.\/)?assets\//.test(indexHtml)) {
  throw new Error("Built index.html does not reference hashed assets");
}

console.log(
  JSON.stringify(
    {
      status: "ok",
      jsAssetCount: jsAssets.length,
      wasmAssetCount: wasmAssets.length,
    },
    null,
    2,
  ),
);

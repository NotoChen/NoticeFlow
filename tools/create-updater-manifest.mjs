import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const repository = process.env.GITHUB_REPOSITORY || "NotoChen/NoticeFlow";
const tag = process.env.GITHUB_REF_NAME || process.env.NOTICEFLOW_RELEASE_TAG;

if (!tag) {
  throw new Error("GITHUB_REF_NAME or NOTICEFLOW_RELEASE_TAG is required");
}

const version = tag.replace(/^v/, "");
const releaseDir = "release-assets";

// Each arch builds into its own target dir. The updater bundle is always
// NoticeFlow.app.tar.gz, so rename it (and the DMG) per arch when copying into
// release-assets, otherwise the two targets would overwrite each other.
const architectures = [
  { updaterKey: "darwin-aarch64", target: "aarch64-apple-darwin", assetSlug: "aarch64" },
  { updaterKey: "darwin-x86_64", target: "x86_64-apple-darwin", assetSlug: "x64" },
];

mkdirSync(releaseDir, { recursive: true });

const platforms = {};

for (const arch of architectures) {
  const bundleDir = `src-tauri/target/${arch.target}/release/bundle`;
  const updaterBundle = join(bundleDir, "macos", "NoticeFlow.app.tar.gz");
  const updaterSignature = `${updaterBundle}.sig`;

  // Tauri encodes the arch into the DMG file name (e.g. x64 / aarch64). Resolve
  // it by globbing the per-target dmg dir instead of hard-coding the upstream
  // slug, which has changed across Tauri versions.
  const dmgDir = join(bundleDir, "dmg");
  const dmgs = existsSync(dmgDir)
    ? readdirSync(dmgDir).filter((f) => f.endsWith(".dmg"))
    : [];
  if (dmgs.length !== 1) {
    throw new Error(`Expected exactly one .dmg in ${dmgDir}, found ${dmgs.length}: ${dmgs.join(", ")}`);
  }
  const dmg = join(dmgDir, dmgs[0]);

  for (const path of [updaterBundle, updaterSignature, dmg]) {
    if (!existsSync(path)) {
      throw new Error(`Missing release artifact for ${arch.target}: ${path}`);
    }
  }

  const dmgAsset = `NoticeFlow_${version}_${arch.assetSlug}.dmg`;
  const updaterAsset = `NoticeFlow_${arch.assetSlug}.app.tar.gz`;

  copyFileSync(dmg, join(releaseDir, dmgAsset));
  copyFileSync(updaterBundle, join(releaseDir, updaterAsset));
  copyFileSync(updaterSignature, join(releaseDir, `${updaterAsset}.sig`));

  platforms[arch.updaterKey] = {
    signature: readFileSync(updaterSignature, "utf8").trim(),
    url: `https://github.com/${repository}/releases/download/${tag}/${updaterAsset}`,
  };
}

const manifest = {
  version,
  pub_date: new Date().toISOString(),
  platforms,
};

writeFileSync(join(releaseDir, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

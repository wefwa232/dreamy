// Sets Dreamy's version in every file that records it, so they can never disagree.
// Usage: npm run bump -- 0.1.2
// Then merge the change to main and push the tag v0.1.2 (see docs/releasing.md).
const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..");
const version = process.argv[2];

if (!/^\d+\.\d+\.\d+$/.test(version || "")) {
  console.error("Give a plain version number such as 0.1.2 (no 'v', no suffix).");
  process.exit(1);
}

// Each entry changes only the version text, so the rest of each file stays exactly as it was.
const files = [
  {
    file: "package.json",
    change: (text) => text.replace(/("version": ")[^"]*(")/, `$1${version}$2`),
  },
  {
    file: "package-lock.json",
    change: (text) =>
      text
        .replace(/("version": ")[^"]*(")/, `$1${version}$2`)
        .replace(/("": \{\s*"name": "[^"]*",\s*"version": ")[^"]*(")/, `$1${version}$2`),
  },
  {
    file: path.join("src-tauri", "tauri.conf.json"),
    change: (text) => text.replace(/("version": ")[^"]*(")/, `$1${version}$2`),
  },
  {
    file: path.join("src-tauri", "Cargo.toml"),
    change: (text) => text.replace(/^version = "[^"]*"/m, `version = "${version}"`),
  },
  {
    file: path.join("src-tauri", "Cargo.lock"),
    change: (text) => text.replace(/(name = "dreamy"\r?\nversion = ")[^"]*(")/, `$1${version}$2`),
  },
];

for (const { file, change } of files) {
  const full = path.join(root, file);
  const before = fs.readFileSync(full, "utf8");
  const after = change(before);
  if (after === before && !before.includes(`"${version}"`) && !before.includes(`version = "${version}"`)) {
    console.error(`Could not find the version in ${file}. Nothing was changed in that file.`);
    process.exit(1);
  }
  fs.writeFileSync(full, after);
  console.log(`${file}: ${version}`);
}

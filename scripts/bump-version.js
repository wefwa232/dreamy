// Sets Dreamy's version in every file that records it, so they can never disagree.
// Usage: npm run bump -- 0.1.2
// Then merge the change to main and push the tag v0.1.2 (see docs/releasing.md).
//
// Checking only, without changing anything: node scripts/bump-version.js --check 0.1.2
// It lists the version each file records and fails if any of them is not 0.1.2.
const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..");

// Every place Dreamy records its version. The release workflow checks all of them.
function recordedVersions() {
  const read = (file) => fs.readFileSync(path.join(root, file), "utf8");
  const packageLock = JSON.parse(read("package-lock.json"));
  const cargoToml = read(path.join("src-tauri", "Cargo.toml")).match(/^version = "([^"]*)"/m);
  const cargoLock = read(path.join("src-tauri", "Cargo.lock")).match(/name = "dreamy"\r?\nversion = "([^"]*)"/);
  return [
    ["package.json", JSON.parse(read("package.json")).version],
    ["package-lock.json", packageLock.version],
    ["package-lock.json (Dreamy's own entry)", packageLock.packages && packageLock.packages[""] && packageLock.packages[""].version],
    [path.join("src-tauri", "tauri.conf.json"), JSON.parse(read(path.join("src-tauri", "tauri.conf.json"))).version],
    [path.join("src-tauri", "Cargo.toml"), cargoToml && cargoToml[1]],
    [path.join("src-tauri", "Cargo.lock"), cargoLock && cargoLock[1]],
  ];
}

if (process.argv[2] === "--check") {
  const expected = process.argv[3];
  if (!/^\d+\.\d+\.\d+$/.test(expected || "")) {
    console.error("Give a plain version number to check against, such as 0.1.2.");
    process.exit(1);
  }
  let allMatch = true;
  for (const [file, found] of recordedVersions()) {
    const matches = found === expected;
    console.log(`${file}: ${found ?? "not found"}${matches ? "" : "  <-- does not match " + expected}`);
    if (!matches) allMatch = false;
  }
  process.exit(allMatch ? 0 : 1);
}

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

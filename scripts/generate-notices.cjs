const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');

const repo = path.resolve(__dirname, '..');
const target = 'x86_64-pc-windows-msvc';
const output = path.join(repo, 'release/legal');
const licenses = path.join(repo, 'licenses');
const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const read = file => fs.readFileSync(file);
const json = file => JSON.parse(read(file).toString('utf8'));
const compare = (a, b) => a < b ? -1 : a > b ? 1 : 0;
const tool = name => {
  const local = path.join(os.homedir(), '.cargo/bin', name + (process.platform === 'win32' ? '.exe' : ''));
  return fs.existsSync(local) ? local : name;
};
function run(name, args) {
  try { return execFileSync(tool(name), args, { cwd: repo, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] }); }
  catch { throw new Error(`${name} ${args[0]} failed. Check the locked dependency/toolchain setup.`); }
}
function inside(root, relative) {
  const file = path.resolve(root, relative);
  if (!file.startsWith(path.resolve(root) + path.sep)) throw new Error('License path must remain inside its package.');
  return file;
}
function licenseFiles(root) {
  const files = [];
  function scan(dir) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory() && !['.git', 'target', 'node_modules'].includes(entry.name)) scan(file);
      else if (entry.isFile() && (/^(?:licen[cs]e|copying|notice)(?:[._-].*)?$/i.test(entry.name) || /^copyright(?:\.(?:txt|md|rst|html))?$/i.test(entry.name)) && !/\.(?:[cm]?js|[cm]?ts|map|rs|c|cpp|h|svg|png)$/i.test(entry.name)) files.push(file);
    }
  }
  scan(root);
  return files.sort(compare);
}

try {
  if (process.platform !== 'win32') throw new Error('Distribution notices are prepared for the Windows x64 build.');
  const manifest = json(path.join(repo, 'package.json'));
  const npmLock = json(path.join(repo, 'package-lock.json'));
  const overrides = json(path.join(licenses, 'overrides.json'));
  const roots = Object.keys(manifest.dependencies).sort(compare);
  if (roots.join('|') !== ['@lucide/vue', '@tauri-apps/api', 'vue'].join('|')) throw new Error('Review the bundled frontend packages after changing application dependencies.');
  const metadata = JSON.parse(run('cargo', ['metadata', '--locked', '--format-version', '1', '--filter-platform', target, '--manifest-path', 'src-tauri/Cargo.toml']));
  const packages = new Map(metadata.packages.map(p => [`${p.name}@${p.version}`, p]));
  function tree(edges) {
    const result = new Set();
    for (const line of run('cargo', ['tree', '--locked', '--offline', '--target', target, '--edges', edges, '--prefix', 'none', '--format', '{p}', '--manifest-path', 'src-tauri/Cargo.toml']).split('\n')) {
      const match = line.match(/^(\S+) v(\S+)/);
      if (match) result.add(`${match[1]}@${match[2]}`);
    }
    return result;
  }
  const rustPackages = tree('normal,build');
  const runtimeCandidates = tree('normal,no-proc-macro');
  const texts = new Map();
  const inventory = [];
  function addText(bytes, reference) {
    if (!bytes.length) throw new Error(`Empty declaration: ${reference}`);
    const digest = hash(bytes);
    if (!texts.has(digest)) texts.set(digest, { text: bytes.toString('utf8'), references: [] });
    texts.get(digest).references.push(reference);
  }
  function overrideFiles(files, label) {
    for (const file of files) {
      const bytes = read(inside(licenses, file.path));
      if (hash(bytes) !== file.sha256) throw new Error(`Declaration checksum mismatch: ${file.path}`);
      addText(bytes, `${label}: ${file.path}\n  Fixed source: ${file.source}\n  Upstream SHA256: ${file.upstreamSha256}; local SHA256: ${file.sha256}`);
    }
  }
  const frontend = ['vue', '@vue/runtime-dom', '@vue/runtime-core', '@vue/reactivity', '@vue/shared', '@tauri-apps/api', '@lucide/vue'];
  for (const name of frontend) {
    const root = path.join(repo, 'node_modules', name);
    const pkg = json(path.join(root, 'package.json'));
    if (pkg.version !== npmLock.packages[`node_modules/${name}`]?.version) throw new Error(`Run npm ci: ${name} does not match the lockfile.`);
    const files = licenseFiles(root);
    if (!files.length) throw new Error(`Missing frontend declaration: ${name}`);
    for (const file of files) addText(read(file), `npm ${name}@${pkg.version}: ${path.relative(root, file).replaceAll('\\', '/')}`);
    inventory.push({ name, version: pkg.version, license: pkg.license, scope: 'frontend runtime', source: `https://www.npmjs.com/package/${name}/v/${pkg.version}` });
  }
  for (const key of [...rustPackages].sort(compare)) {
    const pkg = packages.get(key);
    if (!pkg) throw new Error(`Unresolved locked package: ${key}`);
    if (!pkg.source) {
      if (pkg.id === metadata.resolve.root) continue;
      throw new Error(`Review local third-party source and license: ${key}`);
    }
    if (pkg.source !== 'registry+https://github.com/rust-lang/crates.io-index') throw new Error(`Review non-crates.io source archives: ${key}`);
    if (!pkg.license && !pkg.license_file) throw new Error(`Missing declared license: ${key}`);
    const root = path.dirname(pkg.manifest_path);
    const files = new Set(licenseFiles(root));
    if (pkg.license_file) files.add(inside(root, pkg.license_file));
    for (const file of [...files].sort(compare)) addText(read(file), `crate ${key}: ${path.relative(root, file).replaceAll('\\', '/')}`);
    if (overrides.crates[key]) overrideFiles(overrides.crates[key].files, `crate ${key}`);
    if (![...files].some(file => /^(?:licen[cs]e|copying)/i.test(path.basename(file))) && !overrides.crates[key]) throw new Error(`Missing full license text: ${key}; add a reviewed fixed-source override.`);
    inventory.push({ name: pkg.name, version: pkg.version, license: pkg.license || 'license-file', scope: runtimeCandidates.has(key) ? 'Rust runtime candidate' : 'Rust build/host candidate', source: `https://crates.io/api/v1/crates/${pkg.name}/${pkg.version}/download`, note: overrides.crates[key]?.note });
  }
  for (const component of overrides.components) {
    if (!rustPackages.has(component.crate)) throw new Error(`Review stale binary declaration: ${component.name}`);
    const pkg = packages.get(component.crate);
    const binary = read(inside(path.dirname(pkg.manifest_path), component.binaryPath));
    if (hash(binary) !== component.binarySha256) throw new Error(`Review changed precompiled loader: ${component.name}`);
    overrideFiles(component.files, `${component.name}@${component.version}`);
    inventory.push({ name: component.name, version: component.version, license: component.license, scope: 'precompiled x64 loader', source: component.source, note: `SDK archive SHA256: ${component.archiveSha256}; loader SHA256: ${component.binarySha256}` });
  }
  const rustVersion = run('rustc', ['-Vv']).trim();
  const sysroot = run('rustc', ['--print', 'sysroot']).trim();
  const standardLibrary = path.join(sysroot, 'share/doc/rust/COPYRIGHT-library.html');
  if (!fs.existsSync(standardLibrary)) throw new Error('Rust standard-library notice is missing. Repair the matching official rustc toolchain and retry.');
  const standardBytes = read(standardLibrary);
  const lines = [
    `Third-party notices for SubGauge ${manifest.version} / Windows x64`, '',
    'Scope: seven reviewed frontend runtime packages; the locked Windows normal/build Rust dependency graph, including host code generation.',
    'Runtime candidates exclude proc-macro edges. This inventory is conservative and does not claim every listed package is embedded in the executable.',
    'License declarations are retained in their original language. Fixed-source overrides are checked by SHA256; line endings may be normalized.',
    'SubGauge does not modify the listed dependency sources. Exact crates.io source archives are linked below, including all MPL-2.0 packages.',
    'The application MIT license is supplied separately as LICENSE.',
    'Rust standard-library and associated source/build dependency notices are supplied separately as RUST-STANDARD-LIBRARY-NOTICES.html.',
    `Rust standard-library notice SHA256: ${hash(standardBytes)}`, rustVersion, '', 'PACKAGE INVENTORY', ''
  ];
  for (const pkg of inventory.sort((a, b) => compare(`${a.name}@${a.version}`, `${b.name}@${b.version}`))) {
    lines.push(`${pkg.name}@${pkg.version} | ${pkg.license} | ${pkg.scope}`, `Source: ${pkg.source}`);
    if (pkg.note) lines.push(pkg.note);
    lines.push('');
  }
  lines.push('ORIGINAL LICENSE AND NOTICE TEXTS', '');
  for (const [digest, entry] of [...texts].sort(([a], [b]) => compare(a, b))) lines.push(`===== SHA256 ${digest} =====`, ...entry.references.sort(compare), '', entry.text, '');
  const notice = Buffer.from(lines.join('\n'), 'utf8');
  fs.mkdirSync(output, { recursive: true });
  fs.writeFileSync(path.join(output, 'THIRD-PARTY-NOTICES.txt'), notice);
  fs.writeFileSync(path.join(output, 'RUST-STANDARD-LIBRARY-NOTICES.html'), standardBytes);
  const report = { version: manifest.version, target, frontendPackages: frontend.length, rustPackages: inventory.filter(p => p.scope.startsWith('Rust')).length, components: overrides.components.length, uniqueDeclarationTexts: texts.size, noticeSha256: hash(notice), standardLibrarySha256: hash(standardBytes), inputs: { packageJsonSha256: hash(read(path.join(repo, 'package.json'))), packageLockSha256: hash(read(path.join(repo, 'package-lock.json'))), cargoManifestSha256: hash(read(path.join(repo, 'src-tauri/Cargo.toml'))), cargoLockSha256: hash(read(path.join(repo, 'src-tauri/Cargo.lock'))), overridesSha256: hash(read(path.join(licenses, 'overrides.json'))), generatorSha256: hash(read(__filename)) } };
  fs.writeFileSync(path.join(output, 'notice-manifest.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
} catch (error) { console.error(error.message); process.exitCode = 1; }

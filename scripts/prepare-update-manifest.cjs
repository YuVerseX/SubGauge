// Verify public update artifacts only. This tool never opens signing credentials.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
const { TextDecoder } = require('node:util');

const REPOSITORY = 'YuVerseX/SubGauge';
const MAX_INSTALLER = 48 * 1024 * 1024;
const utf8 = new TextDecoder('utf-8', { fatal: true });

function decodeBase64(text, label) {
  if (typeof text !== 'string' || text.length > 32 * 1024) throw new Error(`${label} has an invalid size.`);
  const trimmed = text.trim();
  if (!trimmed || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(trimmed)) {
    throw new Error(`${label} is not canonical Base64.`);
  }
  const bytes = Buffer.from(trimmed, 'base64');
  if (bytes.toString('base64') !== trimmed) throw new Error(`${label} is not canonical Base64.`);
  return bytes;
}

function envelope(text, count, label) {
  const decoded = utf8.decode(decodeBase64(text, label));
  const lines = decoded.trimEnd().split(/\r?\n/);
  if (lines.length !== count || !lines[0].startsWith('untrusted comment: ')) {
    throw new Error(`${label} has an invalid Minisign envelope.`);
  }
  return lines;
}

function validVersion(version) {
  if (typeof version !== 'string' || version.length > 96) return false;
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(version);
  return Boolean(match && (!match[4] || match[4].split('.').every(part => !/^\d+$/.test(part) || part === '0' || !part.startsWith('0'))));
}

function verifyInstaller(bytes, signatureText, publicKeyText, expectedVersion) {
  if (!Buffer.isBuffer(bytes) || bytes.length === 0 || bytes.length > MAX_INSTALLER) throw new Error('Installer size is invalid.');
  if (!validVersion(expectedVersion)) throw new Error('The expected update version is not valid SemVer.');
  const keyLines = envelope(publicKeyText, 2, 'Public key');
  const keyPacket = decodeBase64(keyLines[1], 'Public key packet');
  const signatureLines = envelope(signatureText, 4, 'Signature');
  const signaturePacket = decodeBase64(signatureLines[1], 'Signature packet');
  const globalSignature = decodeBase64(signatureLines[3], 'Global signature');
  if (keyPacket.length !== 42 || signaturePacket.length !== 74 || globalSignature.length !== 64) {
    throw new Error('Invalid Minisign packet length.');
  }
  const keyAlgorithm = keyPacket.subarray(0, 2).toString('ascii');
  const algorithm = signaturePacket.subarray(0, 2).toString('ascii');
  if (!['Ed', 'ED'].includes(keyAlgorithm) || !['Ed', 'ED'].includes(algorithm)) throw new Error('Unsupported Minisign algorithm.');
  if (!keyPacket.subarray(2, 10).equals(signaturePacket.subarray(2, 10))) throw new Error('Signature uses a different signing key.');
  if (!signatureLines[2].startsWith('trusted comment: ')) throw new Error('Signature trusted comment is missing.');
  const trustedComment = signatureLines[2].slice('trusted comment: '.length);
  const signature = signaturePacket.subarray(10);
  const publicKey = crypto.createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), keyPacket.subarray(10)]),
    format: 'der', type: 'spki',
  });
  const signedData = algorithm === 'ED' ? crypto.createHash('blake2b512').update(bytes).digest() : bytes;
  if (!crypto.verify(null, signedData, publicKey, signature) ||
      !crypto.verify(null, Buffer.concat([signature, Buffer.from(trustedComment, 'utf8')]), publicKey, globalSignature)) {
    throw new Error('Installer signature verification failed.');
  }
  // The global signature authenticates this comment. Never trust it before verification.
  const versions = trustedComment.split('\t').filter(field => field.startsWith('version:')).map(field => field.slice('version:'.length));
  if (versions.length !== 1 || !validVersion(versions[0]) || versions[0] !== expectedVersion) {
    throw new Error('The signed installer version does not match the update version.');
  }
  return { version: expectedVersion, sha256: crypto.createHash('sha256').update(bytes).digest('hex') };
}

function readSmallFile(filename, limit = 32 * 1024) {
  const stat = fs.statSync(filename);
  if (!stat.isFile() || stat.size > limit) throw new Error('Update metadata file is invalid or too large.');
  return fs.readFileSync(filename, 'utf8');
}

function applicationVersion(repo) {
  const packageManifest = JSON.parse(readSmallFile(path.join(repo, 'package.json')));
  const tauriManifest = JSON.parse(readSmallFile(path.join(repo, 'src-tauri', 'tauri.conf.json')));
  const cargo = readSmallFile(path.join(repo, 'src-tauri', 'Cargo.toml'));
  const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
  const version = packageManifest.version;
  if (!validVersion(version) || version !== tauriManifest.version || version !== cargoVersion) throw new Error('npm, Cargo, and Tauri versions must match.');
  const publicKey = readSmallFile(path.join(repo, 'src-tauri', 'updater-public.key')).trim();
  if (publicKey !== tauriManifest.plugins?.updater?.pubkey ||
      tauriManifest.plugins?.updater?.requireSignedVersion !== true ||
      tauriManifest.bundle?.createUpdaterArtifacts !== true) {
    throw new Error('Updater configuration must use the checked public key, signed versions, and updater artifacts.');
  }
  return { version, publicKey };
}

function installerProductVersion(filename) {
  if (process.platform !== 'win32') throw new Error('Checking the NSIS ProductVersion requires Windows.');
  try {
    return execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      "$ErrorActionPreference = 'Stop'; $updateArtifactPath = [Console]::In.ReadToEnd(); [Console]::Out.Write((Get-Item -LiteralPath $updateArtifactPath).VersionInfo.ProductVersion)"],
    { input: filename, encoding: 'utf8', windowsHide: true }).trim();
  } catch {
    throw new Error('Could not inspect the NSIS ProductVersion.');
  }
}

function releaseNotes(repo, version, notesPath) {
  const releaseDocument = notesPath ? path.resolve(notesPath) : path.join(repo, 'docs', 'releases', `${version}.md`);
  if (notesPath || fs.existsSync(releaseDocument)) return readSmallFile(releaseDocument, 16 * 1024).trim();
  const changelog = readSmallFile(path.join(repo, 'CHANGELOG.md'), 128 * 1024);
  const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const section = new RegExp(`^## ${escaped}[^\\n]*\\n([\\s\\S]*?)(?=^## |(?![\\s\\S]))`, 'm').exec(changelog);
  if (!section || section[1].length > 16 * 1024) throw new Error('Provide release notes for the current application version before preparing its update manifest.');
  return section[1].trim();
}

function releaseUrl(version, filename) {
  return `https://github.com/${REPOSITORY}/releases/download/v${encodeURIComponent(version)}/${encodeURIComponent(filename)}`;
}

function prepare(options = {}) {
  const repo = path.resolve(__dirname, '..');
  const { version, publicKey } = applicationVersion(repo);
  const filename = `SubGauge_${version}_x64-setup.exe`;
  const installer = options.installer ? path.resolve(options.installer) : path.join(repo, 'release', filename);
  const signaturePath = `${installer}.sig`;
  if (!fs.existsSync(signaturePath)) throw new Error('Matching installer signature is missing. Unsigned packages cannot become updates.');
  if (path.basename(installer) !== filename) throw new Error('Installer filename does not match the current application version.');
  const stat = fs.statSync(installer);
  if (!stat.isFile() || stat.size === 0 || stat.size > MAX_INSTALLER) throw new Error('Installer size is invalid.');
  const signature = readSmallFile(signaturePath).trim();
  const checked = verifyInstaller(fs.readFileSync(installer), signature, publicKey, version);
  if (installerProductVersion(installer) !== version) throw new Error('Installer ProductVersion does not match the signed application version.');
  if (options.verifyOnly) return checked;
  const platform = { url: releaseUrl(version, filename), signature };
  const manifest = {
    version, notes: releaseNotes(repo, version, options.notes), pub_date: new Date().toISOString(),
    platforms: { 'windows-x86_64-nsis': platform, 'windows-x86_64': platform },
  };
  const output = path.join(repo, 'release', 'update-preview.json');
  const temporary = path.join(path.dirname(output), `.update-preview-${crypto.randomUUID()}.tmp`);
  try {
    fs.writeFileSync(temporary, `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' });
    fs.renameSync(temporary, output);
  } finally {
    if (fs.existsSync(temporary)) fs.unlinkSync(temporary);
  }
  return { ...checked, output };
}

function main(args) {
  const options = {};
  for (let index = 0; index < args.length; index++) {
    const argument = args[index];
    if (argument === '--verify-only') options.verifyOnly = true;
    else if (argument === '--installer' || argument === '--notes') {
      if (!args[index + 1] || args[index + 1].startsWith('--')) throw new Error(`${argument} requires a path.`);
      options[argument.slice(2)] = args[++index];
    } else throw new Error(`Unsupported update preparation option: ${argument}`);
  }
  const checked = prepare(options);
  console.log(`Verified signed SubGauge ${checked.version} installer (SHA256 ${checked.sha256}).`);
  if (checked.output) console.log('Prepared release/update-preview.json for review. The active updates/preview.json feed was not changed.');
}

module.exports = { verifyInstaller, validVersion, releaseUrl, prepare };
if (require.main === module) {
  try { main(process.argv.slice(2)); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}

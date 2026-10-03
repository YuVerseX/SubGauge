// Local release setup: secrets stay outside the repository and never reach stdout.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const repo = path.resolve(__dirname, '..');
const directory = path.join(os.homedir(), '.subgauge', 'signing');
const key = path.join(directory, 'updater.key');
const passwordFile = path.join(directory, 'password.dpapi');
const publicFile = path.join(repo, 'src-tauri', 'updater-public.key');
if (process.platform !== 'win32') throw new Error('Local signing setup requires Windows DPAPI.');
fs.mkdirSync(directory, { recursive: true });
if (fs.existsSync(key) || fs.existsSync(passwordFile)) {
  if (!fs.existsSync(key + '.pub') || !fs.existsSync(passwordFile)) throw new Error('Incomplete signing setup; preserve it and recover manually.');
  const existing = fs.readFileSync(key + '.pub', 'utf8').trim();
  if (fs.existsSync(publicFile) && fs.readFileSync(publicFile, 'utf8').trim() !== existing) throw new Error('Existing public key differs; key rotation must be handled explicitly.');
  fs.writeFileSync(publicFile, existing + '\n');
  console.log('Existing local signing setup is ready. Private key was not changed.');
  process.exit(0);
}
if (fs.existsSync(publicFile)) throw new Error('This repository already has an updater identity. Recover its existing signing key instead of generating a replacement.');
const password = crypto.randomBytes(32).toString('base64url');
const protection = spawnSync('powershell.exe', ['-NoProfile', '-Command', 'Add-Type -AssemblyName System.Security; $bytes = [Convert]::FromBase64String([Console]::In.ReadToEnd()); $protected = [System.Security.Cryptography.ProtectedData]::Protect($bytes, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser); [Console]::Out.Write([Convert]::ToBase64String($protected))'], { input: Buffer.from(password).toString('base64'), encoding: 'utf8', windowsHide: true });
if (protection.status !== 0 || !protection.stdout.trim()) throw new Error('DPAPI password protection failed.');
// Persist recovery material before key generation; a failed run is never overwritten.
fs.writeFileSync(passwordFile, Buffer.from(protection.stdout.trim(), 'base64'), { flag: 'wx' });
// CLI output may contain a private key: capture and discard the entire output.
const signer = spawnSync(process.execPath, [path.join(repo, 'node_modules', '@tauri-apps', 'cli', 'tauri.js'), 'signer', 'generate', '--ci', '--password', password, '--write-keys', key], { cwd: repo, encoding: 'utf8', windowsHide: true });
if (signer.status !== 0 || !fs.existsSync(key + '.pub')) throw new Error('Signing key generation failed; retained recovery files must be inspected without printing secrets.');
fs.writeFileSync(publicFile, fs.readFileSync(key + '.pub', 'utf8').trim() + '\n');
console.log('Local signing setup created. Private key and DPAPI password are outside the repository.');

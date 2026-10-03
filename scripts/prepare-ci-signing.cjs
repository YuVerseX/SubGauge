// Disposable CI identity: never read, reuse, or replace the production signing identity.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');

function assertValidationOnlyEnvironment(args, environment = process.env, platform = process.platform) {
  if (args.length !== 1 || args[0] !== '--validation-only') throw new Error('Explicit --validation-only is required. This tool cannot prepare production releases.');
  if (platform !== 'win32' || environment.CI !== 'true' || environment.GITHUB_ACTIONS !== 'true') {
    throw new Error('Temporary signing identities are restricted to Windows GitHub Actions CI.');
  }
  if (!environment.RUNNER_TEMP || !environment.GITHUB_ENV) throw new Error('The CI temporary directory and environment file are required.');
  if (environment.TAURI_SIGNING_PRIVATE_KEY || environment.TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    throw new Error('Validation CI must not receive production signing credentials. Remove signing Secrets from this validation job.');
  }
}

function inside(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}

function prepare(args) {
  assertValidationOnlyEnvironment(args);
  const repo = fs.realpathSync(path.resolve(__dirname, '..'));
  const runnerTemp = fs.realpathSync(process.env.RUNNER_TEMP);
  const productionDirectory = path.resolve(os.homedir(), '.subgauge', 'signing');
  const environmentFile = path.resolve(process.env.GITHUB_ENV);
  if (inside(repo, runnerTemp) || inside(productionDirectory, runnerTemp) || !inside(runnerTemp, environmentFile)) {
    throw new Error('CI signing files must stay in the runner temporary directory, outside the repository and production signing directory.');
  }
  const directory = path.join(runnerTemp, `subgauge-validation-signing-${crypto.randomUUID()}`);
  if (/[\r\n]/.test(directory)) throw new Error('CI temporary signing path contains invalid separators.');
  fs.mkdirSync(directory);
  const key = path.join(directory, 'updater.key');
  const passwordFile = path.join(directory, 'password.dpapi');
  const password = crypto.randomBytes(32).toString('base64url');
  const passwordBytes = Buffer.from(password, 'utf8');
  let protectedPassword;
  try {
    const protection = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      'Add-Type -AssemblyName System.Security; $ciPassword = [Convert]::FromBase64String([Console]::In.ReadToEnd()); try { $ciProtected = [System.Security.Cryptography.ProtectedData]::Protect($ciPassword, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser); [Console]::Out.Write([Convert]::ToBase64String($ciProtected)) } finally { [Array]::Clear($ciPassword, 0, $ciPassword.Length) }'],
    { input: passwordBytes.toString('base64'), encoding: 'utf8', windowsHide: true });
    if (protection.status !== 0 || !protection.stdout.trim()) throw new Error('CI password protection failed.');
    protectedPassword = Buffer.from(protection.stdout.trim(), 'base64');
  } finally {
    passwordBytes.fill(0);
  }
  fs.writeFileSync(passwordFile, protectedPassword, { flag: 'wx' });
  // The signer can print a private key. Capture and discard both output streams.
  const signer = spawnSync(process.execPath, [path.join(repo, 'node_modules', '@tauri-apps', 'cli', 'tauri.js'),
    'signer', 'generate', '--ci', '--password', password, '--write-keys', key],
  { cwd: repo, encoding: 'utf8', windowsHide: true });
  if (signer.status !== 0 || !fs.existsSync(`${key}.pub`)) throw new Error('Temporary CI signing key generation failed.');
  const publicKey = fs.readFileSync(`${key}.pub`, 'utf8').trim();
  const configPath = path.join(repo, 'src-tauri', 'tauri.conf.json');
  const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
  if (!config.plugins?.updater || config.bundle?.createUpdaterArtifacts !== true) throw new Error('Updater signing configuration is required before CI package validation.');
  config.plugins.updater.pubkey = publicKey;
  // Only this disposable checkout changes; no source commit or active update feed is written.
  fs.writeFileSync(path.join(repo, 'src-tauri', 'updater-public.key'), `${publicKey}\n`);
  fs.writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`);
  const releaseDirectory = path.join(repo, 'release');
  fs.mkdirSync(releaseDirectory, { recursive: true });
  fs.writeFileSync(path.join(releaseDirectory, 'VALIDATION-ONLY.txt'), [
    'SUBGAUGE CI VALIDATION ONLY — NOT AN OFFICIAL RELEASE',
    '',
    'These installers, ZIPs, signatures and manifests were built using an independent, temporary CI signing identity.',
    'They are solely for checking builds, packaging and signature verification. Do not publish them as SubGauge releases or use them in the official update feed.',
    'Their public key differs from the production key. They cannot install or authenticate official SubGauge updates.',
    'Official releases must be built from the unchanged production public key and signed by its separate private key.',
    '',
  ].join('\n'));
  fs.appendFileSync(environmentFile, `SUBGAUGE_SIGNING_DIRECTORY=${directory}\n`);
  console.log('Prepared an isolated, validation-only CI signing identity. Production signing files were not opened or changed.');
}

module.exports = { assertValidationOnlyEnvironment, inside };
if (require.main === module) {
  try { prepare(process.argv.slice(2)); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}

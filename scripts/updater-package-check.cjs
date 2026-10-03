// Security regression vectors use fresh in-memory keys, never production credentials.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const { verifyInstaller, validVersion, releaseUrl } = require('./prepare-update-manifest.cjs');
let passed = 0;
function test(name, run) { run(); passed++; console.log(`PASS ${name}`); }

function fixture(options = {}) {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const publicBytes = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const keyId = crypto.randomBytes(8);
  const bytes = Buffer.from('Public fixture installer bytes; no account information.');
  const algorithm = options.algorithm ?? 'ED';
  const signedData = algorithm === 'ED' ? crypto.createHash('blake2b512').update(bytes).digest() : bytes;
  const signature = crypto.sign(null, signedData, privateKey);
  const comment = options.comment ?? 'timestamp:1\tfile:SubGauge_0.1.8_x64-setup.exe\tversion:0.1.8';
  const global = crypto.sign(null, Buffer.concat([signature, Buffer.from(comment)]), privateKey);
  const signaturePacket = Buffer.concat([Buffer.from(algorithm), keyId, signature]);
  const publicPacket = Buffer.concat([Buffer.from('Ed'), keyId, publicBytes]);
  const publicKeyText = Buffer.from(`untrusted comment: fixture public key\n${publicPacket.toString('base64')}\n`).toString('base64');
  const lines = ['untrusted comment: fixture signature', signaturePacket.toString('base64'), `trusted comment: ${comment}`, global.toString('base64')];
  const signatureText = Buffer.from(lines.join('\n') + '\n').toString('base64');
  return { bytes, publicKeyText, signatureText, lines };
}
function verify(f, bytes = f.bytes, signature = f.signatureText, key = f.publicKeyText) {
  return verifyInstaller(bytes, signature, key, '0.1.8');
}
function wrapped(lines) { return Buffer.from(lines.join('\n') + '\n').toString('base64'); }

test('prehashed Minisign signature and signed version', () => {
  const f = fixture();
  assert.equal(verify(f).version, '0.1.8');
  assert.equal(verify(f).sha256, crypto.createHash('sha256').update(f.bytes).digest('hex'));
});
test('legacy Ed25519 signature compatibility', () => assert.equal(verify(fixture({ algorithm: 'Ed' })).version, '0.1.8'));
test('upstream Minisign vector verifies before the required-version rejection', () => {
  // Public interoperability vector from minisign-verify 0.2.5's verify_prehashed test.
  const publicKey = Buffer.from('untrusted comment: upstream public vector\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n').toString('base64');
  const signature = wrapped([
    'untrusted comment: signature from minisign secret key',
    'RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=',
    'trusted comment: timestamp:1556193335\tfile:test',
    'y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==',
  ]);
  assert.throws(() => verifyInstaller(Buffer.from('test'), signature, publicKey, '0.1.8'), /signed installer version/);
});
test('tampered installer is rejected', () => { const f = fixture(); assert.throws(() => verify(f, Buffer.concat([f.bytes, Buffer.from('changed')])), /verification failed/); });
test('tampered trusted comment is rejected before trusting its version', () => {
  const f = fixture(); const lines = [...f.lines]; lines[2] = lines[2].replace('version:0.1.8', 'version:9.9.9');
  assert.throws(() => verify(f, f.bytes, wrapped(lines)), /verification failed/);
});
test('tampered global signature is rejected', () => {
  const f = fixture(); const lines = [...f.lines]; const global = Buffer.from(lines[3], 'base64'); global[0] ^= 1; lines[3] = global.toString('base64');
  assert.throws(() => verify(f, f.bytes, wrapped(lines)), /verification failed/);
});
test('different signing key is rejected', () => { const f = fixture(); assert.throws(() => verify(f, f.bytes, f.signatureText, fixture().publicKeyText), /different signing key/); });
test('genuinely signed previous version is rejected', () => assert.throws(() => verify(fixture({ comment: 'timestamp:1\tversion:0.1.7' })), /signed installer version/));
test('signed comment without version is rejected', () => assert.throws(() => verify(fixture({ comment: 'timestamp:1\tfile:fixture.exe' })), /signed installer version/));
test('multiple signed version fields are rejected', () => assert.throws(() => verify(fixture({ comment: 'timestamp:1\tversion:0.1.8\tversion:0.1.8' })), /signed installer version/));
test('unsupported signature algorithm is rejected', () => {
  const f = fixture(); const lines = [...f.lines]; const packet = Buffer.from(lines[1], 'base64'); packet.write('XX'); lines[1] = packet.toString('base64');
  assert.throws(() => verify(f, f.bytes, wrapped(lines)), /Unsupported Minisign/);
});
test('noncanonical Base64 is rejected', () => { const f = fixture(); assert.throws(() => verify(f, f.bytes, f.signatureText + '!'), /canonical Base64/); });
test('extra envelope lines are rejected', () => { const f = fixture(); assert.throws(() => verify(f, f.bytes, wrapped([...f.lines, 'extra'])), /envelope/); });
test('empty installer is rejected', () => { const f = fixture(); assert.throws(() => verify(f, Buffer.alloc(0)), /size/); });
test('oversized installer is rejected', () => { const f = fixture(); assert.throws(() => verify(f, Buffer.alloc(48 * 1024 * 1024 + 1)), /size/); });
test('strict SemVer', () => {
  for (const version of ['0.1.8', '1.2.3-preview.1', '1.2.3+build.01']) assert.equal(validVersion(version), true);
  for (const version of ['01.2.3', '1.2', 'v0.1.8', '1.2.3-preview.01', '0.1.8\n', '1.2.3-']) assert.equal(validVersion(version), false);
});
test('release URL stays in the official repository and escapes each segment', () => {
  assert.equal(releaseUrl('0.1.8', 'SubGauge_0.1.8_x64-setup.exe'), 'https://github.com/YuVerseX/SubGauge/releases/download/v0.1.8/SubGauge_0.1.8_x64-setup.exe');
  assert.ok(new URL(releaseUrl('0.1.8', '../other.exe')).pathname.startsWith('/YuVerseX/SubGauge/releases/download/v0.1.8/'));
});
console.log(`${passed} update package regressions passed.`);

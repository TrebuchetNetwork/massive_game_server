const fs = require('fs');
const os = require('os');
const path = require('path');

function defaultSmsCaptureFile(name = 'mgs-e2e-otp') {
  return path.join(os.tmpdir(), `${name}-${process.pid}.txt`);
}

function prepareSmsCaptureFile(filePath = defaultSmsCaptureFile()) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  try {
    fs.unlinkSync(filePath);
  } catch (_) {}
  return filePath;
}

async function waitForSmsCode(filePath, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const raw = fs.readFileSync(filePath, 'utf8');
      const match = raw.match(/\b(\d{6})\b/);
      if (match) {
        return match[1];
      }
    } catch (_) {}
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`Timed out waiting for OTP code in ${filePath}`);
}

module.exports = {
  defaultSmsCaptureFile,
  prepareSmsCaptureFile,
  waitForSmsCode,
};

import fs from "node:fs";
import path from "node:path";
import { createHash, randomBytes } from "node:crypto";
import { deflateSync, inflateSync } from "node:zlib";

const FORMAT_VERSION = 1;
const CONTROL_ID = "spinal-phase0b-control";
const INBOUND_ATTRIBUTE = "data-spinal-phase0b-inbound";
const OUTBOUND_ATTRIBUTE = "data-spinal-phase0b-outbound";
const OBSERVATION_ID = "spinal-phase0b-observation";
const COMPLETE_ATTRIBUTE = "data-spinal-phase0b-complete";
const STATE_ATTRIBUTE = "data-spinal-phase0b-state";
const MAX_PROTOCOL_BYTES = 64 * 1024;
const MAX_OUTER_TERMINAL_BYTES = 8 * 1024 * 1024 + 64 * 1024;
const MAX_PNG_BYTES = 4 * 1024 * 1024;
const WIDTH = 640;
const HEIGHT = 480;
const RGB_CHANNELS = 3;
const RGBA_CHANNELS = 4;
const EXPECTED_RGB_SCANLINE_BYTES = HEIGHT * (1 + WIDTH * RGB_CHANNELS);
const EXPECTED_RGBA_SCANLINE_BYTES = HEIGHT * (1 + WIDTH * RGBA_CHANNELS);
const MAX_GENERATION = Number.MAX_SAFE_INTEGER;
const TARGET_TIMEOUT_MS = 10_000;
const CONNECT_TIMEOUT_MS = 10_000;
const COMMAND_TIMEOUT_MS = 10_000;
const CONTROL_TIMEOUT_MS = 30_000;
const POLL_MS = 20;
const PNG_SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const HEX_256 = /^[0-9a-f]{64}$/;
const SAFE_FILE = /^[a-z0-9][a-z0-9.-]*$/;
const SCHEDULE = Object.freeze([
  Object.freeze({ sequence: 0, source: "current", sample: "sway-start" }),
  Object.freeze({ sequence: 1, source: "proposed", sample: "sway-start" }),
  Object.freeze({ sequence: 2, source: "current", sample: "sway-middle" }),
  Object.freeze({ sequence: 3, source: "proposed", sample: "sway-middle" }),
  Object.freeze({ sequence: 4, source: "current", sample: "sway-alternate-skin" }),
  Object.freeze({ sequence: 5, source: "proposed", sample: "sway-alternate-skin" }),
  Object.freeze({ sequence: 6, source: "current", sample: "sway-end" }),
  Object.freeze({ sequence: 7, source: "proposed", sample: "sway-end" }),
]);
const SCREENSHOT_FILES = Object.freeze(SCHEDULE.map(({ sequence, source, sample }) => (
  `${String(sequence).padStart(2, "0")}-${sample}-${source}.png`
)));
const TERMINAL_FILE = "phase0b-browser-terminal.json";
const MANIFEST_FILE = "phase0b-browser-capture-manifest.json";
const FAILURE_FILE = "phase0b-browser-capture-failure.json";

function fail(message) {
  throw new Error(message);
}

function exactKeys(value, expected, context) {
  if (value === null || Array.isArray(value) || typeof value !== "object") {
    fail(`${context} must be an object`);
  }
  const actual = Object.keys(value);
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    fail(`${context} has non-canonical fields or field order`);
  }
}

function requireHex(value, context) {
  if (typeof value !== "string" || !HEX_256.test(value)) {
    fail(`${context} must be 64 lowercase hexadecimal characters`);
  }
  return value;
}

function requireGeneration(value, context) {
  if (!Number.isSafeInteger(value) || value < 1 || value > MAX_GENERATION) {
    fail(`${context} must be a positive safe integer`);
  }
  return value;
}

function identityJson(identity) {
  return `{"manifest_sha256":"${identity.manifest_sha256}","content_sha256":"${identity.content_sha256}"}`;
}

function runtimeSourcesJson(sources) {
  return `{"current":${identityJson(sources.current)},"proposed":${identityJson(sources.proposed)}}`;
}

function challengeJson(nonce) {
  return `{"format_version":1,"message":"challenge","nonce":"${nonce}"}`;
}

function challengeAckJson(document) {
  return `{"format_version":1,"message":"challenge_ack","nonce":"${document.nonce}","runtime_sources":${runtimeSourcesJson(document.runtime_sources)}}`;
}

function requestJson(document, message = "screenshot_request") {
  return `{"format_version":1,"message":"${message}","nonce":"${document.nonce}","sequence":${document.sequence},"source":"${document.source}","sample":"${document.sample}","runtime_identity":${identityJson(document.runtime_identity)},"frame_revision":${document.frame_revision},"acknowledged_play_revision":${document.acknowledged_play_revision},"acknowledged_seek_revision":${document.acknowledged_seek_revision}`;
}

function screenshotRequestJson(document) {
  return `${requestJson(document)}}`;
}

function screenshotAckJson(document) {
  return `${requestJson(document, "screenshot_ack")},"png_byte_length":${document.png_byte_length},"png_sha256":"${document.png_sha256}"}`;
}

function receiptJson(receipt) {
  return `{"sequence":${receipt.sequence},"source":"${receipt.source}","sample":"${receipt.sample}","runtime_identity":${identityJson(receipt.runtime_identity)},"frame_revision":${receipt.frame_revision},"acknowledged_play_revision":${receipt.acknowledged_play_revision},"acknowledged_seek_revision":${receipt.acknowledged_seek_revision},"png_byte_length":${receipt.png_byte_length},"png_sha256":"${receipt.png_sha256}"}`;
}

function terminalJson(document) {
  return `{"format_version":1,"state":"complete","nonce":"${document.nonce}","runtime_sources":${runtimeSourcesJson(document.runtime_sources)},"screenshots":[${document.screenshots.map(receiptJson).join(",")}]}`;
}

function captureManifestJson(nonce, terminal, screenshots) {
  const entries = screenshots.map((item) => (
    `{"sequence":${item.sequence},"file":"${item.file}","byte_length":${item.png_byte_length},"sha256":"${item.png_sha256}"}`
  )).join(",");
  return `{"format_version":1,"artifact_kind":"phase0b_browser_capture","evidence_class":"non_representative_rehearsal","gate_eligible":false,"nonce":"${nonce}","terminal":{"file":"${TERMINAL_FILE}","byte_length":${terminal.byte_length},"sha256":"${terminal.sha256}"},"screenshots":[${entries}]}`;
}

function parseCanonical(raw, context) {
  if (typeof raw !== "string") fail(`${context} must be a string`);
  const byteLength = Buffer.byteLength(raw, "utf8");
  if (byteLength < 1 || byteLength > MAX_PROTOCOL_BYTES) {
    fail(`${context} length is outside 1..=${MAX_PROTOCOL_BYTES}`);
  }
  try {
    return JSON.parse(raw);
  } catch (_error) {
    fail(`${context} is not valid JSON`);
  }
}

function validateIdentity(value, context) {
  exactKeys(value, ["manifest_sha256", "content_sha256"], context);
  return {
    manifest_sha256: requireHex(value.manifest_sha256, `${context}.manifest_sha256`),
    content_sha256: requireHex(value.content_sha256, `${context}.content_sha256`),
  };
}

function validateRuntimeSources(value, context) {
  exactKeys(value, ["current", "proposed"], context);
  return {
    current: validateIdentity(value.current, `${context}.current`),
    proposed: validateIdentity(value.proposed, `${context}.proposed`),
  };
}

function identitiesEqual(left, right) {
  return left.manifest_sha256 === right.manifest_sha256
    && left.content_sha256 === right.content_sha256;
}

function runtimeSourcesEqual(left, right) {
  return identitiesEqual(left.current, right.current)
    && identitiesEqual(left.proposed, right.proposed);
}

function parseChallengeAck(raw, nonce) {
  const value = parseCanonical(raw, "challenge acknowledgement");
  exactKeys(value, ["format_version", "message", "nonce", "runtime_sources"], "challenge acknowledgement");
  if (value.format_version !== FORMAT_VERSION || value.message !== "challenge_ack") {
    fail("challenge acknowledgement has the wrong protocol kind");
  }
  if (value.nonce !== nonce) fail("challenge acknowledgement nonce does not match");
  const result = {
    format_version: FORMAT_VERSION,
    message: "challenge_ack",
    nonce,
    runtime_sources: validateRuntimeSources(value.runtime_sources, "runtime_sources"),
  };
  if (raw !== challengeAckJson(result)) fail("challenge acknowledgement is not canonical compact JSON");
  return result;
}

function validateRequestShape(value, context) {
  exactKeys(value, [
    "format_version", "message", "nonce", "sequence", "source", "sample",
    "runtime_identity", "frame_revision", "acknowledged_play_revision",
    "acknowledged_seek_revision",
  ], context);
  if (value.format_version !== FORMAT_VERSION || value.message !== "screenshot_request") {
    fail(`${context} has the wrong protocol kind`);
  }
  if (!Number.isSafeInteger(value.sequence)) fail(`${context}.sequence must be a safe integer`);
  if (typeof value.source !== "string" || typeof value.sample !== "string") {
    fail(`${context} source and sample must be strings`);
  }
  return {
    format_version: FORMAT_VERSION,
    message: "screenshot_request",
    nonce: requireHex(value.nonce, `${context}.nonce`),
    sequence: value.sequence,
    source: value.source,
    sample: value.sample,
    runtime_identity: validateIdentity(value.runtime_identity, `${context}.runtime_identity`),
    frame_revision: requireGeneration(value.frame_revision, `${context}.frame_revision`),
    acknowledged_play_revision: requireGeneration(
      value.acknowledged_play_revision,
      `${context}.acknowledged_play_revision`,
    ),
    acknowledged_seek_revision: requireGeneration(
      value.acknowledged_seek_revision,
      `${context}.acknowledged_seek_revision`,
    ),
  };
}

function parseScreenshotRequest(raw, expectedSequence, nonce, runtimeSources) {
  const value = parseCanonical(raw, "screenshot request");
  const request = validateRequestShape(value, "screenshot request");
  if (raw !== screenshotRequestJson(request)) fail("screenshot request is not canonical compact JSON");
  if (request.nonce !== nonce) fail("screenshot request nonce changed");
  const scheduled = SCHEDULE[expectedSequence];
  if (!scheduled) fail("screenshot request arrived after the fixed schedule");
  if (request.sequence !== expectedSequence) {
    fail(`screenshot request sequence ${request.sequence} is replayed or out of order`);
  }
  if (request.source !== scheduled.source || request.sample !== scheduled.sample) {
    fail("screenshot request does not match the fixed sample-major schedule");
  }
  if (!identitiesEqual(request.runtime_identity, runtimeSources[scheduled.source])) {
    fail("screenshot request runtime identity does not match the challenge acknowledgement");
  }
  return request;
}

function parseReceipt(value, expected, context) {
  exactKeys(value, [
    "sequence", "source", "sample", "runtime_identity", "frame_revision",
    "acknowledged_play_revision", "acknowledged_seek_revision", "png_byte_length",
    "png_sha256",
  ], context);
  const receipt = {
    sequence: value.sequence,
    source: value.source,
    sample: value.sample,
    runtime_identity: validateIdentity(value.runtime_identity, `${context}.runtime_identity`),
    frame_revision: value.frame_revision,
    acknowledged_play_revision: value.acknowledged_play_revision,
    acknowledged_seek_revision: value.acknowledged_seek_revision,
    png_byte_length: value.png_byte_length,
    png_sha256: requireHex(value.png_sha256, `${context}.png_sha256`),
  };
  for (const field of [
    "sequence", "source", "sample", "frame_revision", "acknowledged_play_revision",
    "acknowledged_seek_revision", "png_byte_length", "png_sha256",
  ]) {
    if (receipt[field] !== expected[field]) fail(`${context}.${field} does not match the local receipt`);
  }
  if (!identitiesEqual(receipt.runtime_identity, expected.runtime_identity)) {
    fail(`${context}.runtime_identity does not match the local receipt`);
  }
  return receipt;
}

function parseTerminal(raw, nonce, runtimeSources, localReceipts) {
  const value = parseCanonical(raw, "terminal capture document");
  exactKeys(value, ["format_version", "state", "nonce", "runtime_sources", "screenshots"], "terminal capture document");
  if (value.format_version !== FORMAT_VERSION || value.state !== "complete") {
    fail("terminal capture document has the wrong protocol kind");
  }
  if (value.nonce !== nonce) fail("terminal capture nonce changed");
  const sources = validateRuntimeSources(value.runtime_sources, "terminal runtime_sources");
  if (!runtimeSourcesEqual(sources, runtimeSources)) fail("terminal runtime identities changed");
  if (!Array.isArray(value.screenshots) || value.screenshots.length !== SCHEDULE.length) {
    fail("terminal capture document must contain exactly eight screenshot receipts");
  }
  const screenshots = value.screenshots.map((receipt, index) => (
    parseReceipt(receipt, localReceipts[index], `screenshots[${index}]`)
  ));
  const result = {
    format_version: FORMAT_VERSION,
    state: "complete",
    nonce,
    runtime_sources: sources,
    screenshots,
  };
  if (raw !== terminalJson(result)) fail("terminal capture document is not canonical compact JSON");
  return result;
}

function validateCompactJson(raw, context) {
  let cursor = 0;
  const topLevelValues = new Map();
  const topLevelArrays = new Map();
  const invalid = () => fail(`${context} is not compact JSON or contains duplicate keys`);
  const parseString = () => {
    if (raw[cursor] !== '"') invalid();
    const start = cursor++;
    while (cursor < raw.length) {
      const code = raw.charCodeAt(cursor);
      if (code < 0x20) invalid();
      if (raw[cursor] === '"') {
        cursor += 1;
        try {
          return JSON.parse(raw.slice(start, cursor));
        } catch (_error) {
          invalid();
        }
      }
      if (raw[cursor++] === "\\") {
        const escape = raw[cursor++];
        if (escape === "u") {
          if (!/^[0-9a-fA-F]{4}$/.test(raw.slice(cursor, cursor + 4))) invalid();
          cursor += 4;
        } else if (!'"\\/bfnrt'.includes(escape)) {
          invalid();
        }
      }
    }
    invalid();
  };
  const parseValue = (depth) => {
    if (depth > 128) invalid();
    if (raw[cursor] === "{") {
      cursor += 1;
      const keys = new Set();
      if (raw[cursor] === "}") { cursor += 1; return null; }
      while (cursor < raw.length) {
        const key = parseString();
        if (keys.has(key)) invalid();
        keys.add(key);
        if (raw[cursor++] !== ":") invalid();
        const valueStart = cursor;
        const arrayElements = parseValue(depth + 1);
        if (depth === 0) {
          topLevelValues.set(key, raw.slice(valueStart, cursor));
          if (arrayElements !== null) topLevelArrays.set(key, arrayElements);
        }
        const separator = raw[cursor++];
        if (separator === "}") return null;
        if (separator !== ",") invalid();
      }
      invalid();
    }
    if (raw[cursor] === "[") {
      cursor += 1;
      const elements = [];
      if (raw[cursor] === "]") { cursor += 1; return elements; }
      while (cursor < raw.length) {
        const elementStart = cursor;
        parseValue(depth + 1);
        elements.push(raw.slice(elementStart, cursor));
        const separator = raw[cursor++];
        if (separator === "]") return elements;
        if (separator !== ",") invalid();
      }
      invalid();
    }
    if (raw[cursor] === '"') { parseString(); return null; }
    for (const literal of ["true", "false", "null"]) {
      if (raw.startsWith(literal, cursor)) { cursor += literal.length; return null; }
    }
    const number = /-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/y;
    number.lastIndex = cursor;
    const matched = number.exec(raw);
    if (!matched || !Number.isFinite(Number(matched[0]))) invalid();
    cursor = number.lastIndex;
    return null;
  };
  parseValue(0);
  if (cursor !== raw.length) invalid();
  return { topLevelValues, topLevelArrays };
}

function parseOuterTerminal(raw, nonce, runtimeSources, localReceipts) {
  if (typeof raw !== "string" || Buffer.byteLength(raw, "utf8") > MAX_OUTER_TERMINAL_BYTES) {
    fail("outer terminal document is missing or too large");
  }
  const compact = validateCompactJson(raw, "outer terminal document");
  const { topLevelValues } = compact;
  let value;
  try {
    value = JSON.parse(raw);
  } catch (_error) {
    fail("outer terminal document is not valid JSON");
  }
  exactKeys(
    value,
    ["format_version", "state", "browser_capture", "observations"],
    "outer terminal document",
  );
  if (value.format_version !== 2 || value.state !== "complete") {
    fail("outer terminal document has the wrong schema or state");
  }
  if (topLevelValues.get("format_version") !== "2") {
    fail("outer terminal format_version is not a canonical integer");
  }
  const nestedRaw = topLevelValues.get("browser_capture");
  if (typeof nestedRaw !== "string") fail("outer terminal document is missing browser_capture");
  parseTerminal(nestedRaw, nonce, runtimeSources, localReceipts);
  if (!Array.isArray(value.observations) || value.observations.length !== SCHEDULE.length) {
    fail("outer terminal document must contain exactly eight observations");
  }
  const rawObservations = compact.topLevelArrays.get("observations");
  if (!rawObservations || rawObservations.length !== SCHEDULE.length) {
    fail("outer terminal document has invalid raw observation framing");
  }
  for (let index = 0; index < SCHEDULE.length; index += 1) {
    const observation = value.observations[index];
    const receipt = localReceipts[index];
    exactKeys(observation, [
      "source", "sample", "frame_revision", "acknowledged_play_revision",
      "acknowledged_seek_revision", "frame",
    ], `observations[${index}]`);
    for (const field of [
      "source", "sample", "frame_revision", "acknowledged_play_revision",
      "acknowledged_seek_revision",
    ]) {
      if (observation[field] !== receipt[field]) {
        fail(`observations[${index}].${field} does not match its screenshot receipt`);
      }
    }
    const rawFields = validateCompactJson(
      rawObservations[index],
      `observations[${index}]`,
    ).topLevelValues;
    for (const field of [
      "source", "sample", "frame_revision", "acknowledged_play_revision",
      "acknowledged_seek_revision",
    ]) {
      if (rawFields.get(field) !== JSON.stringify(receipt[field])) {
        fail(`observations[${index}].${field} is not in canonical binding form`);
      }
    }
    if (observation.frame === null || Array.isArray(observation.frame)
      || typeof observation.frame !== "object") {
      fail(`observations[${index}].frame must be an object`);
    }
  }
  return value;
}

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function validatePng(bytes) {
  if (!Buffer.isBuffer(bytes)) fail("screenshot is not a byte buffer");
  if (bytes.length < 1 || bytes.length > MAX_PNG_BYTES) {
    fail(`screenshot PNG length is outside 1..=${MAX_PNG_BYTES}`);
  }
  if (bytes.length < 33 || !bytes.subarray(0, 8).equals(PNG_SIGNATURE)) {
    fail("screenshot has an invalid PNG signature or header");
  }
  let cursor = 8;
  let stage = "header";
  let chunkIndex = 0;
  let channels = 0;
  let expectedScanlineBytes = 0;
  const seenMetadata = new Set();
  const imageData = [];
  while (cursor < bytes.length) {
    if (cursor + 12 > bytes.length) fail("screenshot PNG chunk framing is truncated");
    const length = bytes.readUInt32BE(cursor);
    const typeStart = cursor + 4;
    const dataStart = cursor + 8;
    const dataEnd = dataStart + length;
    const chunkEnd = dataEnd + 4;
    if (!Number.isSafeInteger(chunkEnd) || chunkEnd > bytes.length) {
      fail("screenshot PNG chunk length is invalid");
    }
    const type = bytes.subarray(typeStart, dataStart).toString("latin1");
    const storedCrc = bytes.readUInt32BE(dataEnd);
    if (crc32(bytes.subarray(typeStart, dataEnd)) !== storedCrc) {
      fail(`screenshot PNG chunk ${chunkIndex} has an invalid CRC`);
    }
    if (type === "IHDR" && stage === "header" && cursor === 8 && length === 13) {
      const width = bytes.readUInt32BE(dataStart);
      const height = bytes.readUInt32BE(dataStart + 4);
      if (width !== WIDTH || height !== HEIGHT) fail("screenshot PNG dimensions are not 640x480");
      const bitDepth = bytes[dataStart + 8];
      const colorType = bytes[dataStart + 9];
      const compression = bytes[dataStart + 10];
      const filter = bytes[dataStart + 11];
      const interlace = bytes[dataStart + 12];
      if (
        bitDepth !== 8
        || (colorType !== 2 && colorType !== 6)
        || compression !== 0
        || filter !== 0
        || interlace !== 0
      ) {
        fail("screenshot PNG is not static non-interlaced RGB8 or RGBA8");
      }
      channels = colorType === 2 ? RGB_CHANNELS : RGBA_CHANNELS;
      expectedScanlineBytes = colorType === 2
        ? EXPECTED_RGB_SCANLINE_BYTES
        : EXPECTED_RGBA_SCANLINE_BYTES;
      stage = "metadata";
    } else if (
      stage === "metadata"
      && ((type === "cHRM" && length === 32)
        || (type === "gAMA" && length === 4)
        || (type === "sBIT" && length === channels)
        || (type === "sRGB" && length === 1)
        || (type === "bKGD" && length === 6)
        || (type === "pHYs" && length === 9))
    ) {
      if (seenMetadata.has(type)) fail(`screenshot PNG contains duplicate metadata chunk ${type}`);
      if (type === "gAMA" && bytes.readUInt32BE(dataStart) === 0) {
        fail("screenshot PNG gAMA value must be nonzero");
      }
      if (
        type === "sBIT"
        && bytes.subarray(dataStart, dataEnd).some((value) => value < 1 || value > 8)
      ) {
        fail("screenshot PNG sBIT values must be in 1..=8");
      }
      if (type === "sRGB" && bytes[dataStart] > 3) {
        fail("screenshot PNG sRGB rendering intent must be in 0..=3");
      }
      if (type === "pHYs" && bytes[dataStart + 8] > 1) {
        fail("screenshot PNG pHYs unit must be 0 or 1");
      }
      seenMetadata.add(type);
      // This is the exact ancillary-chunk allowlist in pixel_compare.rs.
    } else if ((stage === "metadata" || stage === "image") && type === "IDAT" && length > 0) {
      stage = "image";
      imageData.push(bytes.subarray(dataStart, dataEnd));
    } else if (stage === "image" && type === "IEND" && length === 0) {
      if (chunkEnd !== bytes.length) fail("screenshot PNG has bytes after IEND");
      const compressed = Buffer.concat(imageData);
      let inflated;
      try {
        inflated = inflateSync(compressed, {
          info: true,
          maxOutputLength: expectedScanlineBytes + 1,
        });
      } catch (_error) {
        fail("screenshot PNG image data is invalid");
      }
      if (inflated.engine.bytesWritten !== compressed.length) {
        fail("screenshot PNG has trailing compressed input");
      }
      const decoded = inflated.buffer;
      if (decoded.length !== expectedScanlineBytes) fail("screenshot PNG decoded size is invalid");
      const scanlineBytes = 1 + WIDTH * channels;
      for (let row = 0; row < HEIGHT; row += 1) {
        if (decoded[row * scanlineBytes] > 4) fail("screenshot PNG uses an invalid row filter");
      }
      return;
    } else {
      fail("screenshot PNG contains an unsupported chunk or chunk order");
    }
    cursor = chunkEnd;
    chunkIndex += 1;
  }
  fail("screenshot PNG is missing IEND");
}

function decodeScreenshot(value) {
  if (typeof value !== "string" || value.length === 0 || value.length % 4 !== 0) {
    fail("CDP screenshot is not canonical base64");
  }
  if (value.length > Math.ceil(MAX_PNG_BYTES / 3) * 4 + 4 || !/^[A-Za-z0-9+/]*={0,2}$/.test(value)) {
    fail("CDP screenshot base64 is invalid or too large");
  }
  const bytes = Buffer.from(value, "base64");
  if (bytes.toString("base64") !== value) fail("CDP screenshot base64 is not canonical");
  validatePng(bytes);
  return bytes;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function parseCli(args) {
  if (args.length === 1 && args[0] === "--self-test") return { mode: "self-test" };
  if (args.length !== 3) {
    fail("usage: node tools/spinal-phase0b-cdp.js PORT EXPECTED_PAGE_URL NEW_OUTPUT_DIR");
  }
  if (!/^[1-9][0-9]{0,4}$/.test(args[0])) fail("PORT must be a canonical decimal port");
  const port = Number(args[0]);
  if (!Number.isInteger(port) || port > 65535) fail("PORT must be in 1..=65535");
  let expectedUrl;
  try {
    expectedUrl = new URL(args[1]);
  } catch (_error) {
    fail("EXPECTED_PAGE_URL must be an absolute URL");
  }
  if (
    expectedUrl.protocol !== "http:"
    || expectedUrl.hostname !== "127.0.0.1"
    || expectedUrl.username !== ""
    || expectedUrl.password !== ""
    || expectedUrl.hash !== ""
    || expectedUrl.href !== args[1]
  ) {
    fail("EXPECTED_PAGE_URL must be a canonical http://127.0.0.1 URL without credentials or fragment");
  }
  if (!args[2] || args[2].includes("\0")) fail("NEW_OUTPUT_DIR must be a non-empty filesystem path");
  if (!path.isAbsolute(args[2]) || path.resolve(args[2]) !== args[2] || path.parse(args[2]).root === args[2]) {
    fail("NEW_OUTPUT_DIR must be an absolute normalized non-root path");
  }
  const parent = path.dirname(args[2]);
  let parentStat;
  let realParent;
  try {
    parentStat = fs.lstatSync(parent);
    realParent = fs.realpathSync.native(parent);
  } catch (_error) {
    fail("NEW_OUTPUT_DIR parent must already exist");
  }
  if (!parentStat.isDirectory() || parentStat.isSymbolicLink() || realParent !== parent) {
    fail("NEW_OUTPUT_DIR parent must be a canonical directory without symlink aliases");
  }
  if (fs.existsSync(args[2])) fail("NEW_OUTPUT_DIR must not already exist");
  return { mode: "capture", port, expectedPageUrl: args[1], outputDir: args[2] };
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function fetchTargets(port) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 1_000);
  try {
    const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: controller.signal });
    if (!response.ok) fail(`CDP target endpoint returned HTTP ${response.status}`);
    const value = await response.json();
    if (!Array.isArray(value)) fail("CDP target endpoint did not return an array");
    return value;
  } finally {
    clearTimeout(timer);
  }
}

function validateDebuggerUrl(value, port) {
  let url;
  try {
    url = new URL(value);
  } catch (_error) {
    fail("matching CDP page has an invalid debugger URL");
  }
  if (
    url.protocol !== "ws:"
    || url.hostname !== "127.0.0.1"
    || url.port !== String(port)
    || url.username !== ""
    || url.password !== ""
    || url.hash !== ""
  ) {
    fail("matching CDP page debugger URL is not confined to 127.0.0.1 and PORT");
  }
  return url.href;
}

async function findExactPage(port, expectedPageUrl) {
  const deadline = Date.now() + TARGET_TIMEOUT_MS;
  let lastCount = 0;
  while (Date.now() < deadline) {
    try {
      const targets = await fetchTargets(port);
      const matches = targets.filter((target) => target?.type === "page" && target.url === expectedPageUrl);
      lastCount = matches.length;
      if (matches.length > 1) fail("CDP published multiple pages with EXPECTED_PAGE_URL");
      if (matches.length === 1) {
        if (typeof matches[0].webSocketDebuggerUrl !== "string") {
          fail("matching CDP page does not expose a debugger URL");
        }
        return validateDebuggerUrl(matches[0].webSocketDebuggerUrl, port);
      }
    } catch (error) {
      if (error instanceof Error && /multiple pages|matching CDP page/.test(error.message)) throw error;
    }
    await delay(POLL_MS);
  }
  fail(`CDP did not publish exactly one matching page (last match count ${lastCount})`);
}

function closeSocketQuietly(socket) {
  try {
    socket.close();
  } catch (_error) {
    // A connecting socket can race its transport setup; a later open handler
    // retries closure before it can retain the Node process.
  }
}

async function connectCdp(url) {
  const socket = new WebSocket(url);
  await new Promise((resolve, reject) => {
    let settled = false;
    let timer;
    const rejectAndClose = (message) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      closeSocketQuietly(socket);
      reject(new Error(message));
    };
    timer = setTimeout(
      () => rejectAndClose("CDP WebSocket connection timed out"),
      CONNECT_TIMEOUT_MS,
    );
    socket.addEventListener("open", () => {
      if (settled) {
        closeSocketQuietly(socket);
        return;
      }
      settled = true;
      clearTimeout(timer);
      resolve();
    }, { once: true });
    socket.addEventListener(
      "error",
      () => rejectAndClose("CDP WebSocket connection failed"),
      { once: true },
    );
  });
  let nextId = 0;
  const pending = new Map();
  const rejectAll = (error) => {
    for (const entry of pending.values()) {
      clearTimeout(entry.timer);
      entry.reject(error);
    }
    pending.clear();
  };
  socket.addEventListener("close", () => rejectAll(new Error("CDP WebSocket closed")));
  socket.addEventListener("error", () => rejectAll(new Error("CDP WebSocket failed")));
  socket.addEventListener("message", (event) => {
    let message;
    try {
      message = JSON.parse(typeof event.data === "string" ? event.data : Buffer.from(event.data).toString("utf8"));
    } catch (_error) {
      rejectAll(new Error("CDP sent an invalid JSON message"));
      return;
    }
    if (!Number.isInteger(message.id) || !pending.has(message.id)) return;
    const entry = pending.get(message.id);
    pending.delete(message.id);
    clearTimeout(entry.timer);
    if (message.error) entry.reject(new Error(message.error.message || "CDP command failed"));
    else entry.resolve(message.result);
  });
  const command = (method, params = {}, timeoutMs = COMMAND_TIMEOUT_MS) => new Promise((resolve, reject) => {
    if (socket.readyState !== WebSocket.OPEN) return reject(new Error(`CDP is not open for ${method}`));
    const id = ++nextId;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`CDP command timed out: ${method}`));
    }, timeoutMs);
    pending.set(id, { resolve, reject, timer });
    socket.send(JSON.stringify({ id, method, params }));
  });
  return {
    command,
    close() {
      rejectAll(new Error("CDP connection closed by capture driver"));
      socket.close();
    },
  };
}

function attributesMap(flat) {
  if (!Array.isArray(flat) || flat.length % 2 !== 0) fail("CDP returned malformed DOM attributes");
  const result = new Map();
  for (let index = 0; index < flat.length; index += 2) result.set(flat[index], flat[index + 1]);
  return result;
}

async function findReservedNodes(command) {
  const deadline = Date.now() + CONTROL_TIMEOUT_MS;
  let lastControlCount = 0;
  let lastObservationCount = 0;
  while (Date.now() < deadline) {
    const root = await command("DOM.getDocument", { depth: 0, pierce: false });
    const control = await command("DOM.querySelectorAll", {
      nodeId: root.root.nodeId,
      selector: `#${CONTROL_ID}`,
    });
    const observation = await command("DOM.querySelectorAll", {
      nodeId: root.root.nodeId,
      selector: `#${OBSERVATION_ID}`,
    });
    lastControlCount = control.nodeIds.length;
    lastObservationCount = observation.nodeIds.length;
    if (lastControlCount > 1) fail(`reserved control id ${CONTROL_ID} is ambiguous`);
    if (lastObservationCount > 1) {
      fail(`reserved observation id ${OBSERVATION_ID} is ambiguous`);
    }
    if (lastControlCount === 1 && lastObservationCount === 1) {
      return {
        controlNodeId: control.nodeIds[0],
        observationNodeId: observation.nodeIds[0],
      };
    }
    await delay(POLL_MS);
  }
  fail(
    `reserved Phase 0B nodes did not both appear (control ${lastControlCount}, observation ${lastObservationCount})`,
  );
}

async function observationText(command, nodeId) {
  const described = await command("DOM.describeNode", { nodeId, depth: 1, pierce: false });
  const children = described.node?.children;
  if (
    !Array.isArray(children)
    || children.length !== 1
    || children[0].nodeName !== "#text"
    || children[0].nodeType !== 3
    || !Number.isSafeInteger(children[0].nodeId)
    || children[0].nodeId < 1
  ) {
    fail("outer terminal element must contain exactly one text node");
  }
  const resolved = await command("DOM.resolveNode", { nodeId: children[0].nodeId });
  const remote = resolved.object;
  if (
    remote?.type !== "object"
    || remote.subtype !== "node"
    || typeof remote.objectId !== "string"
    || remote.objectId === ""
  ) {
    fail("CDP did not resolve the outer terminal text node");
  }
  let primaryError;
  try {
    const exact = await command("Runtime.callFunctionOn", {
      objectId: remote.objectId,
      functionDeclaration: "function () { return this.nodeValue; }",
      arguments: [],
      silent: true,
      returnByValue: true,
      awaitPromise: false,
    });
    if (
      exact.exceptionDetails
      || exact.result?.type !== "string"
      || exact.result.subtype !== undefined
      || typeof exact.result.value !== "string"
    ) {
      fail("CDP did not return the exact outer terminal text value");
    }
    if (Buffer.byteLength(exact.result.value, "utf8") > MAX_OUTER_TERMINAL_BYTES) {
      fail("outer terminal document is missing or too large");
    }
    return exact.result.value;
  } catch (error) {
    primaryError = error;
    throw error;
  } finally {
    try {
      await command("Runtime.releaseObject", { objectId: remote.objectId });
    } catch (releaseError) {
      if (primaryError === undefined) throw releaseError;
    }
  }
}

async function observationProgress(command, nodeId) {
  const result = await command("DOM.getAttributes", { nodeId });
  const attributes = attributesMap(result.attributes);
  return {
    complete: attributes.get(COMPLETE_ATTRIBUTE),
    state: attributes.get(STATE_ATTRIBUTE),
  };
}

async function waitForOuterTerminal(command, nodeId) {
  const deadline = Date.now() + CONTROL_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const { complete, state } = await observationProgress(command, nodeId);
    if (complete === "true") {
      const raw = await observationText(command, nodeId);
      if (state === "complete") return raw;
      fail("browser published a terminal Phase 0B error");
    }
    if (complete !== "false" || (state !== "running" && state !== "awaiting_capture")) {
      fail("outer terminal element has invalid progress attributes");
    }
    await delay(POLL_MS);
  }
  fail("browser did not publish the outer Phase 0B terminal document");
}

async function getOutbound(command, nodeId) {
  const result = await command("DOM.getAttributes", { nodeId });
  return attributesMap(result.attributes).get(OUTBOUND_ATTRIBUTE);
}

async function setInbound(command, nodeId, value) {
  if (Buffer.byteLength(value, "utf8") > MAX_PROTOCOL_BYTES) fail("inbound protocol message is too large");
  await command("DOM.setAttributeValue", { nodeId, name: INBOUND_ATTRIBUTE, value });
}

function boundedDomDiagnostic(value) {
  if (typeof value !== "string") return "absent";
  const clipped = value.length > 512 ? `${value.slice(0, 509)}...` : value;
  return `${Buffer.byteLength(value, "utf8")} bytes:${clipped}`;
}

async function waitForOutbound(command, nodeId, observationNodeId, previous) {
  const deadline = Date.now() + CONTROL_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const value = await getOutbound(command, nodeId);
    if (typeof value === "string" && value !== "" && value !== previous) return value;
    const progress = await observationProgress(command, observationNodeId);
    if (progress.complete === "true") {
      const terminal = await observationText(command, observationNodeId);
      fail(
        `browser published terminal ${progress.state} before the next control message (${boundedDomDiagnostic(terminal)})`,
      );
    }
    if (
      progress.complete !== "false"
      || (progress.state !== "running" && progress.state !== "awaiting_capture")
    ) {
      fail("outer terminal element has invalid progress attributes while awaiting control output");
    }
    await delay(POLL_MS);
  }
  const attributes = attributesMap(
    (await command("DOM.getAttributes", { nodeId })).attributes,
  );
  const progress = await observationProgress(command, observationNodeId);
  const terminal = await observationText(command, observationNodeId);
  fail(
    "browser did not publish the next Phase 0B control message "
      + `(inbound ${boundedDomDiagnostic(attributes.get(INBOUND_ATTRIBUTE))}; `
      + `outbound ${boundedDomDiagnostic(attributes.get(OUTBOUND_ATTRIBUTE))}; `
      + `terminal complete=${progress.complete}, state=${progress.state}, `
      + `text ${boundedDomDiagnostic(terminal)})`,
  );
}

async function compositorBarrier(command) {
  const expression = "new Promise((resolve)=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true))))";
  const result = await command("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails || result.result?.value !== true) fail("fixed two-frame compositor barrier failed");
}

function createOutputDirectory(outputDir) {
  fs.mkdirSync(outputDir, { recursive: false, mode: 0o700 });
  fs.chmodSync(outputDir, 0o700);
  const stat = fs.lstatSync(outputDir);
  if (!stat.isDirectory() || stat.isSymbolicLink() || (stat.mode & 0o777) !== 0o700) {
    fail("NEW_OUTPUT_DIR is not a newly created owner-private directory");
  }
}

function writeCreateOnly(outputDir, file, bytes) {
  if (!SAFE_FILE.test(file) || path.basename(file) !== file) fail("internal output filename is unsafe");
  const flags = fs.constants.O_CREAT | fs.constants.O_EXCL | fs.constants.O_WRONLY
    | (fs.constants.O_NOFOLLOW || 0);
  const descriptor = fs.openSync(path.join(outputDir, file), flags, 0o600);
  try {
    fs.fchmodSync(descriptor, 0o600);
    fs.writeFileSync(descriptor, bytes);
    fs.fsyncSync(descriptor);
  } finally {
    fs.closeSync(descriptor);
  }
}

function boundedFailureJson(nonce, error) {
  const raw = error instanceof Error ? error.message : String(error);
  const message = raw.length > 4096 ? `${raw.slice(0, 4093)}...` : raw;
  return JSON.stringify({
    format_version: 1,
    state: "failed",
    evidence_class: "non_representative_rehearsal",
    gate_eligible: false,
    nonce: HEX_256.test(nonce || "") ? nonce : null,
    error: { message },
  });
}

async function capture(options) {
  if (typeof fetch !== "function" || typeof WebSocket !== "function" || typeof AbortController !== "function") {
    fail("Node.js must provide fetch, WebSocket, and AbortController");
  }
  createOutputDirectory(options.outputDir);
  const nonce = randomBytes(32).toString("hex");
  let cdp;
  try {
    const debuggerUrl = await findExactPage(options.port, options.expectedPageUrl);
    cdp = await connectCdp(debuggerUrl);
    const { command } = cdp;
    await command("Page.enable");
    await command("DOM.enable");
    await command("Runtime.enable");
    await command("Emulation.setDeviceMetricsOverride", {
      width: WIDTH,
      height: HEIGHT,
      deviceScaleFactor: 1,
      mobile: false,
    });
    const { controlNodeId: nodeId, observationNodeId } = await findReservedNodes(command);
    let outbound = await getOutbound(command, nodeId);
    await setInbound(command, nodeId, challengeJson(nonce));
    outbound = await waitForOutbound(command, nodeId, observationNodeId, outbound);
    const challengeAck = parseChallengeAck(outbound, nonce);
    const receipts = [];
    const files = [];
    for (let sequence = 0; sequence < SCHEDULE.length; sequence += 1) {
      const requestRaw = await waitForOutbound(command, nodeId, observationNodeId, outbound);
      const request = parseScreenshotRequest(
        requestRaw,
        sequence,
        nonce,
        challengeAck.runtime_sources,
      );
      outbound = requestRaw;
      await compositorBarrier(command);
      const captured = await command("Page.captureScreenshot", {
        format: "png",
        fromSurface: true,
        captureBeyondViewport: false,
      });
      const png = decodeScreenshot(captured.data);
      const digest = sha256(png);
      const file = SCREENSHOT_FILES[sequence];
      writeCreateOnly(options.outputDir, file, png);
      const receipt = {
        sequence,
        source: request.source,
        sample: request.sample,
        runtime_identity: request.runtime_identity,
        frame_revision: request.frame_revision,
        acknowledged_play_revision: request.acknowledged_play_revision,
        acknowledged_seek_revision: request.acknowledged_seek_revision,
        png_byte_length: png.length,
        png_sha256: digest,
      };
      receipts.push(receipt);
      files.push({ sequence, file, png_byte_length: png.length, png_sha256: digest });
      await setInbound(command, nodeId, screenshotAckJson({ ...request, ...receipt, nonce }));
    }
    const captureCompleteRaw = await waitForOutbound(
      command,
      nodeId,
      observationNodeId,
      outbound,
    );
    parseTerminal(captureCompleteRaw, nonce, challengeAck.runtime_sources, receipts);
    const terminalRaw = await waitForOuterTerminal(command, observationNodeId);
    const terminalBytes = Buffer.from(terminalRaw, "utf8");
    parseOuterTerminal(terminalRaw, nonce, challengeAck.runtime_sources, receipts);
    writeCreateOnly(options.outputDir, TERMINAL_FILE, terminalBytes);
    const terminal = { byte_length: terminalBytes.length, sha256: sha256(terminalBytes) };
    const manifest = captureManifestJson(nonce, terminal, files);
    writeCreateOnly(options.outputDir, MANIFEST_FILE, Buffer.from(manifest, "utf8"));
    process.stdout.write(`${manifest}\n`);
  } catch (error) {
    try {
      writeCreateOnly(options.outputDir, FAILURE_FILE, Buffer.from(boundedFailureJson(nonce, error), "utf8"));
    } catch (_diagnosticError) {
      // Existing partial artifacts remain owner-private; never replace one.
    }
    throw error;
  } finally {
    cdp?.close();
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(`self-test failed: ${message}`);
}

function assertThrows(action, pattern, message) {
  try {
    action();
  } catch (error) {
    assert(pattern.test(String(error?.message)), message);
    return;
  }
  throw new Error(`self-test failed: ${message} (did not throw)`);
}

function pngChunk(type, data) {
  const typeBytes = Buffer.isBuffer(type) ? Buffer.from(type) : Buffer.from(type, "ascii");
  if (typeBytes.length !== 4) throw new Error("test PNG chunk type must contain four bytes");
  const result = Buffer.alloc(12 + data.length);
  result.writeUInt32BE(data.length, 0);
  typeBytes.copy(result, 4);
  data.copy(result, 8);
  result.writeUInt32BE(crc32(Buffer.concat([typeBytes, data])), 8 + data.length);
  return result;
}

function testPng(
  width = WIDTH,
  height = HEIGHT,
  colorType = 6,
  compressedSuffix = Buffer.alloc(0),
  rowFilter = 0,
  bitDepth = 8,
) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr.set([bitDepth, colorType, 0, 0, 0], 8);
  const channels = colorType === 6 ? RGBA_CHANNELS : RGB_CHANNELS;
  const scanlineBytes = 1 + width * channels;
  const rows = Buffer.alloc(height * scanlineBytes);
  for (let row = 0; row < height; row += 1) rows[row * scanlineBytes] = rowFilter;
  return Buffer.concat([
    PNG_SIGNATURE,
    pngChunk("IHDR", ihdr),
    pngChunk("IDAT", Buffer.concat([deflateSync(rows), compressedSuffix])),
    pngChunk("IEND", Buffer.alloc(0)),
  ]);
}

function withMetadataChunks(png, chunks) {
  const ihdrEnd = PNG_SIGNATURE.length + 12 + 13;
  return Buffer.concat([
    png.subarray(0, ihdrEnd),
    ...chunks.map(([type, data]) => pngChunk(type, data)),
    png.subarray(ihdrEnd),
  ]);
}

function withMetadataChunk(png, type, data) {
  return withMetadataChunks(png, [[type, data]]);
}

async function runSelfTests() {
  const nonce = "1".repeat(64);
  const identity = (digit) => ({ manifest_sha256: digit.repeat(64), content_sha256: digit.repeat(64) });
  const runtimeSources = { current: identity("2"), proposed: identity("3") };
  const output = path.join(fs.realpathSync.native(process.cwd()), `.phase0b-cdp-self-test-${process.pid}`);
  assert(parseCli(["--self-test"]).mode === "self-test", "self-test CLI");
  assert(parseCli(["9222", "http://127.0.0.1:8080/", output]).port === 9222, "capture CLI");
  assertThrows(() => parseCli(["022", "http://127.0.0.1/", output]), /canonical decimal/, "non-canonical port");
  assertThrows(() => parseCli(["22", "http://localhost/", output]), /127\.0\.0\.1/, "non-loopback page URL");
  assertThrows(() => parseCli(["22", "http://127.0.0.1/", "relative"]), /absolute normalized/, "relative output");
  assertThrows(() => parseCli([]), /usage:/, "missing CLI arguments");
  let closeCalls = 0;
  closeSocketQuietly({ close() { closeCalls += 1; } });
  assert(closeCalls === 1, "connection timeout helper closes its socket");
  closeSocketQuietly({ close() { throw new Error("connecting"); } });

  const challengeAck = { format_version: 1, message: "challenge_ack", nonce, runtime_sources: runtimeSources };
  const ackRaw = challengeAckJson(challengeAck);
  assert(parseChallengeAck(ackRaw, nonce).nonce === nonce, "canonical challenge acknowledgement");
  assertThrows(() => parseChallengeAck(`${ackRaw.slice(0, -1)},"nonce":"${nonce}"}`, nonce), /canonical/, "duplicate JSON key");
  assertThrows(() => parseChallengeAck(JSON.stringify(challengeAck, null, 2), nonce), /not canonical compact/, "pretty JSON");
  const reordered = `{"message":"challenge_ack","format_version":1,"nonce":"${nonce}","runtime_sources":${runtimeSourcesJson(runtimeSources)}}`;
  assertThrows(() => parseChallengeAck(reordered, nonce), /field order/, "reordered JSON");

  const request = {
    format_version: 1,
    message: "screenshot_request",
    nonce,
    sequence: 0,
    source: "current",
    sample: "sway-start",
    runtime_identity: runtimeSources.current,
    frame_revision: 1,
    acknowledged_play_revision: 2,
    acknowledged_seek_revision: 3,
  };
  const requestRaw = screenshotRequestJson(request);
  assert(parseScreenshotRequest(requestRaw, 0, nonce, runtimeSources).sequence === 0, "fixed request");
  assertThrows(() => parseScreenshotRequest(requestRaw, 1, nonce, runtimeSources), /replayed or out of order/, "replayed sequence");
  const future = { ...request, sequence: 2, source: "current", sample: "sway-middle" };
  assertThrows(() => parseScreenshotRequest(screenshotRequestJson(future), 0, nonce, runtimeSources), /replayed or out of order/, "future sequence");
  const wrongIdentity = { ...request, runtime_identity: runtimeSources.proposed };
  assertThrows(() => parseScreenshotRequest(screenshotRequestJson(wrongIdentity), 0, nonce, runtimeSources), /runtime identity/, "wrong identity");
  const wrongNonce = { ...request, nonce: "9".repeat(64) };
  assertThrows(() => parseScreenshotRequest(screenshotRequestJson(wrongNonce), 0, nonce, runtimeSources), /nonce changed/, "changed nonce");
  const zeroGeneration = { ...request, frame_revision: 0 };
  assertThrows(() => parseScreenshotRequest(screenshotRequestJson(zeroGeneration), 0, nonce, runtimeSources), /positive safe integer/, "zero generation");

  const screenshotAck = {
    ...request,
    message: "screenshot_ack",
    png_byte_length: 1_024,
    png_sha256: "4".repeat(64),
  };
  const screenshotAckRaw = screenshotAckJson(screenshotAck);
  const screenshotAckValue = parseCanonical(screenshotAckRaw, "screenshot acknowledgement");
  exactKeys(screenshotAckValue, [
    "format_version", "message", "nonce", "sequence", "source", "sample",
    "runtime_identity", "frame_revision", "acknowledged_play_revision",
    "acknowledged_seek_revision", "png_byte_length", "png_sha256",
  ], "screenshot acknowledgement");
  assert(screenshotAckValue.message === "screenshot_ack", "screenshot acknowledgement kind");
  assert(screenshotAckValue.nonce === nonce, "screenshot acknowledgement nonce");
  assert(screenshotAckValue.png_byte_length === 1_024, "screenshot acknowledgement byte length");
  assert(screenshotAckValue.png_sha256 === "4".repeat(64), "screenshot acknowledgement digest");
  assert(
    screenshotAckRaw === `${requestJson(screenshotAck, "screenshot_ack")},"png_byte_length":1024,"png_sha256":"${"4".repeat(64)}"}`,
    "screenshot acknowledgement canonical wire",
  );

  const rgbaPng = testPng(WIDTH, HEIGHT, 6);
  const rgbPng = testPng(WIDTH, HEIGHT, 2);
  validatePng(rgbaPng);
  validatePng(rgbPng);
  validatePng(withMetadataChunk(rgbPng, "sBIT", Buffer.alloc(RGB_CHANNELS, 8)));
  validatePng(withMetadataChunk(rgbaPng, "sBIT", Buffer.alloc(RGBA_CHANNELS, 8)));
  validatePng(withMetadataChunk(rgbPng, "bKGD", Buffer.alloc(6)));
  validatePng(withMetadataChunk(rgbaPng, "bKGD", Buffer.alloc(6)));
  const validMetadata = [
    ["cHRM", Buffer.alloc(32)],
    ["gAMA", Buffer.from([0, 0, 0, 1])],
    ["sBIT", Buffer.alloc(RGBA_CHANNELS, 8)],
    ["sRGB", Buffer.from([0])],
    ["bKGD", Buffer.alloc(6)],
    ["pHYs", Buffer.alloc(9)],
  ];
  validatePng(withMetadataChunks(rgbaPng, validMetadata));
  for (const [type, data] of validMetadata) {
    assertThrows(
      () => validatePng(withMetadataChunks(rgbaPng, [[type, data], [type, data]])),
      /duplicate metadata chunk/,
      `duplicate PNG ${type}`,
    );
  }
  assertThrows(
    () => validatePng(withMetadataChunk(rgbaPng, "gAMA", Buffer.alloc(4))),
    /gAMA value must be nonzero/,
    "zero PNG gamma",
  );
  for (const invalidSbit of [0, 9]) {
    assertThrows(
      () => validatePng(
        withMetadataChunk(rgbaPng, "sBIT", Buffer.alloc(RGBA_CHANNELS, invalidSbit)),
      ),
      /sBIT values must be in 1\.\.=8/,
      `PNG sBIT value ${invalidSbit}`,
    );
  }
  assertThrows(
    () => validatePng(withMetadataChunk(rgbaPng, "sRGB", Buffer.from([4]))),
    /sRGB rendering intent must be in 0\.\.=3/,
    "PNG sRGB rendering intent",
  );
  const invalidPhys = Buffer.alloc(9);
  invalidPhys[8] = 2;
  assertThrows(
    () => validatePng(withMetadataChunk(rgbaPng, "pHYs", invalidPhys)),
    /pHYs unit must be 0 or 1/,
    "PNG physical unit",
  );
  assertThrows(
    () => validatePng(withMetadataChunk(rgbaPng, "tIME", Buffer.alloc(7))),
    /unsupported chunk or chunk order/,
    "volatile PNG time metadata",
  );
  assertThrows(
    () => validatePng(withMetadataChunk(rgbPng, "sBIT", Buffer.alloc(RGBA_CHANNELS, 8))),
    /unsupported chunk or chunk order/,
    "RGB PNG sBIT length",
  );
  assertThrows(
    () => validatePng(withMetadataChunk(rgbaPng, "sBIT", Buffer.alloc(RGB_CHANNELS, 8))),
    /unsupported chunk or chunk order/,
    "RGBA PNG sBIT length",
  );
  const png = rgbaPng;
  const corrupt = Buffer.from(rgbaPng);
  corrupt[corrupt.length - 1] ^= 1;
  assertThrows(() => validatePng(corrupt), /invalid CRC/, "PNG CRC");
  for (const unsupportedColorType of [0, 1, 3, 4, 5, 7]) {
    assertThrows(
      () => validatePng(testPng(WIDTH, HEIGHT, unsupportedColorType)),
      /RGB8 or RGBA8/,
      `PNG color type ${unsupportedColorType}`,
    );
  }
  assertThrows(
    () => validatePng(testPng(WIDTH, HEIGHT, 6, Buffer.alloc(0), 0, 16)),
    /RGB8 or RGBA8/,
    "PNG bit depth",
  );
  assertThrows(
    () => validatePng(testPng(WIDTH, HEIGHT, 2, Buffer.alloc(0), 5)),
    /invalid row filter/,
    "RGB PNG row filter",
  );
  assertThrows(
    () => validatePng(testPng(WIDTH, HEIGHT, 6, Buffer.alloc(0), 5)),
    /invalid row filter/,
    "RGBA PNG row filter",
  );
  const highBitIendAlias = Buffer.from([0xc9, 0x45, 0x4e, 0x44]);
  const highBitChunkPng = Buffer.concat([
    rgbaPng.subarray(0, rgbaPng.length - 12),
    pngChunk(highBitIendAlias, Buffer.alloc(0)),
    rgbaPng.subarray(rgbaPng.length - 12),
  ]);
  assertThrows(
    () => validatePng(highBitChunkPng),
    /unsupported chunk or chunk order/,
    "high-bit PNG chunk alias",
  );
  assertThrows(() => validatePng(testPng(WIDTH - 1, HEIGHT)), /dimensions/, "PNG dimensions");
  assertThrows(
    () => validatePng(testPng(WIDTH, HEIGHT, 6, Buffer.from([0xde, 0xad]))),
    /trailing compressed input/,
    "PNG trailing compressed input",
  );
  assertThrows(() => validatePng(Buffer.concat([png, Buffer.from("x")])), /after IEND/, "PNG trailing bytes");
  assertThrows(() => validatePng(Buffer.alloc(MAX_PNG_BYTES + 1)), /length/, "PNG size bound");

  const receipts = SCHEDULE.map((scheduled) => ({
    ...scheduled,
    runtime_identity: runtimeSources[scheduled.source],
    frame_revision: scheduled.sequence + 1,
    acknowledged_play_revision: scheduled.sequence + 11,
    acknowledged_seek_revision: scheduled.sequence + 21,
    png_byte_length: png.length,
    png_sha256: sha256(png),
  }));
  const terminalDocument = {
    format_version: 1,
    state: "complete",
    nonce,
    runtime_sources: runtimeSources,
    screenshots: receipts,
  };
  const terminalRaw = terminalJson(terminalDocument);
  parseTerminal(terminalRaw, nonce, runtimeSources, receipts);
  const outerRaw = JSON.stringify({
    format_version: 2,
    state: "complete",
    browser_capture: terminalDocument,
    observations: receipts.map((receipt) => ({
      source: receipt.source,
      sample: receipt.sample,
      frame_revision: receipt.frame_revision,
      acknowledged_play_revision: receipt.acknowledged_play_revision,
      acknowledged_seek_revision: receipt.acknowledged_seek_revision,
      frame: { format_version: 1 },
    })),
  });
  parseOuterTerminal(outerRaw, nonce, runtimeSources, receipts);
  assertThrows(() => parseOuterTerminal(` ${outerRaw}`, nonce, runtimeSources, receipts), /compact JSON/, "outer whitespace");
  assertThrows(
    () => parseOuterTerminal(outerRaw.replace('"format_version":2', '"format_version":2.0'), nonce, runtimeSources, receipts),
    /canonical integer/,
    "outer version with a decimal spelling",
  );
  const duplicateOuter = `${outerRaw.slice(0, -1)},"state":"complete"}`;
  assertThrows(() => parseOuterTerminal(duplicateOuter, nonce, runtimeSources, receipts), /duplicate keys/, "outer duplicate key");
  assertThrows(
    () => parseOuterTerminal(outerRaw.replace('"sequence":0,', '"sequence":0.0,'), nonce, runtimeSources, receipts),
    /canonical compact/,
    "nested protocol integer with a decimal spelling",
  );
  assertThrows(
    () => parseOuterTerminal(outerRaw.replace('"frame_revision":1,', '"frame_revision":1e0,'), nonce, runtimeSources, receipts),
    /canonical compact/,
    "nested protocol integer with an exponent spelling",
  );
  const observationsPivot = outerRaw.indexOf('"observations":[');
  assert(observationsPivot > 0, "outer test document observations pivot");
  const noncanonicalObservation = outerRaw.slice(0, observationsPivot)
    + outerRaw.slice(observationsPivot).replace('"frame_revision":1,', '"frame_revision":1.0,');
  assertThrows(
    () => parseOuterTerminal(noncanonicalObservation, nonce, runtimeSources, receipts),
    /canonical binding form/,
    "semantic observation integer with a decimal spelling",
  );
  const reorderedOuter = `{"state":"complete","format_version":2,"browser_capture":${terminalRaw},"observations":${JSON.stringify(JSON.parse(outerRaw).observations)}}`;
  assertThrows(() => parseOuterTerminal(reorderedOuter, nonce, runtimeSources, receipts), /field order/, "outer field order");

  const escapedCompact = String.raw`{"quote":"\"","reverse_solidus":"\\","solidus":"\/","controls":"\b\f\n\r\t","unicode":"\u0061","surrogate_pair":"\ud83d\ude00"}`;
  validateCompactJson(escapedCompact, "escaped compact JSON test");
  assert(
    JSON.stringify(JSON.parse(escapedCompact))
      === '{"quote":"\\\"","reverse_solidus":"\\\\","solidus":"/","controls":"\\b\\f\\n\\r\\t","unicode":"a","surrogate_pair":"😀"}',
    "compact JSON escape round trip",
  );
  assertThrows(
    () => validateCompactJson(String.raw`{"a":1,"\u0061":2}`, "escaped duplicate test"),
    /duplicate keys/,
    "escaped duplicate JSON key",
  );
  for (const [raw, label] of [
    [String.raw`{"value":"\x00"}`, "invalid escape"],
    [String.raw`{"value":"\u123"}`, "short Unicode escape"],
    ['{"value":"line\nbreak"}', "raw newline"],
    ['{"value":"dangling' + "\\", "dangling reverse solidus"],
  ]) {
    assertThrows(
      () => validateCompactJson(raw, `${label} test`),
      /compact JSON/,
      label,
    );
  }

  const longSemanticRaw = JSON.stringify({
    source: "proposed",
    sample: "sway-alternate-skin",
    frame_revision: 4,
    acknowledged_play_revision: 5,
    acknowledged_seek_revision: 15,
    frame: {
      format_version: 1,
      default_skin: "default",
      skin_layers: ["alternate"],
      bones: Array.from({ length: 160 }, (_unused, ordinal) => ({
        ordinal,
        name: `body-line-${ordinal}`,
        local: {
          translation: [0.0, 0.0],
          rotation_radians: 0.061086524,
          scale: [1.0, 1.0],
          shear_radians: [0.0, 0.0],
        },
      })),
    },
  });
  assert(Buffer.byteLength(longSemanticRaw, "utf8") > 10_003, "long semantic JSON exceeds describeNode clipping boundary");
  validateCompactJson(longSemanticRaw, "long semantic JSON test");
  assert(JSON.stringify(JSON.parse(longSemanticRaw)) === longSemanticRaw, "long semantic JSON round trip");

  const cdpMethods = [];
  const exactObservation = await observationText(async (method, params) => {
    cdpMethods.push(method);
    if (method === "DOM.describeNode") {
      assert(params.nodeId === 101 && params.depth === 1 && params.pierce === false, "describe exact observation node");
      return {
        node: {
          children: [{
            nodeId: 102,
            nodeType: 3,
            nodeName: "#text",
            nodeValue: `${longSemanticRaw.slice(0, 10_000)}…`,
          }],
        },
      };
    }
    if (method === "DOM.resolveNode") {
      assert(params.nodeId === 102, "resolve exact observation text child");
      return { object: { type: "object", subtype: "node", objectId: "terminal-text-1" } };
    }
    if (method === "Runtime.callFunctionOn") {
      assert(params.objectId === "terminal-text-1", "read resolved observation text child");
      assert(params.functionDeclaration === "function () { return this.nodeValue; }", "fixed nodeValue reader");
      assert(params.returnByValue === true && params.awaitPromise === false, "nodeValue read by value");
      return { result: { type: "string", value: longSemanticRaw } };
    }
    if (method === "Runtime.releaseObject") {
      assert(params.objectId === "terminal-text-1", "release resolved observation text child");
      return {};
    }
    throw new Error(`unexpected mock CDP method ${method}`);
  }, 101);
  assert(exactObservation === longSemanticRaw, "exact long observation value");
  assert(
    cdpMethods.join(",")
      === "DOM.describeNode,DOM.resolveNode,Runtime.callFunctionOn,Runtime.releaseObject",
    "exact observation CDP lifecycle",
  );

  let releasedAfterReadError = false;
  let primaryReadError;
  try {
    await observationText(async (method) => {
      if (method === "DOM.describeNode") {
        return { node: { children: [{ nodeId: 202, nodeType: 3, nodeName: "#text" }] } };
      }
      if (method === "DOM.resolveNode") {
        return { object: { type: "object", subtype: "node", objectId: "terminal-text-2" } };
      }
      if (method === "Runtime.callFunctionOn") {
        return { exceptionDetails: { text: "mock read failure" }, result: { type: "undefined" } };
      }
      if (method === "Runtime.releaseObject") {
        releasedAfterReadError = true;
        throw new Error("mock release failure");
      }
      throw new Error(`unexpected mock CDP method ${method}`);
    }, 201);
  } catch (error) {
    primaryReadError = error;
  }
  assert(releasedAfterReadError, "resolved observation object released after read failure");
  assert(
    primaryReadError instanceof Error
      && primaryReadError.message === "CDP did not return the exact outer terminal text value",
    "release failure does not mask primary observation read failure",
  );
  const files = receipts.map((receipt, index) => ({
    sequence: index,
    file: SCREENSHOT_FILES[index],
    png_byte_length: receipt.png_byte_length,
    png_sha256: receipt.png_sha256,
  }));
  const terminal = { byte_length: Buffer.byteLength(terminalRaw), sha256: sha256(terminalRaw) };
  const firstManifest = captureManifestJson(nonce, terminal, files);
  const secondManifest = captureManifestJson(nonce, terminal, files);
  assert(firstManifest === secondManifest, "manifest determinism");
  assert(firstManifest.includes('"gate_eligible":false'), "manifest is gate-ineligible");
  const fixedFiles = SCHEDULE.map(({ sequence }) => ({
    sequence,
    file: SCREENSHOT_FILES[sequence],
    png_byte_length: 100 + sequence,
    png_sha256: String((sequence + 5) % 10).repeat(64),
  }));
  const fixedManifest = captureManifestJson(
    nonce,
    { byte_length: 17, sha256: "4".repeat(64) },
    fixedFiles,
  );
  assert(sha256(fixedManifest) === "0efa5e779d475458a66a823ebf61bcd39243c644f294aca591227ff68e28d8e4", "fixed manifest vector");
  process.stdout.write("spinal-phase0b-cdp self-test: ok\n");
}

let options;
try {
  options = parseCli(process.argv.slice(2));
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 2;
}
if (options?.mode === "self-test") {
  runSelfTests().catch((error) => {
    console.error(error instanceof Error ? error.stack : String(error));
    process.exitCode = 1;
  });
} else if (options?.mode === "capture") {
  capture(options).catch((error) => {
    console.error(error instanceof Error ? error.stack : String(error));
    process.exitCode = 1;
  });
}

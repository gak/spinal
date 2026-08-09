import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { deflateSync, inflateSync } from "node:zlib";

const FORMAT_VERSION = 1;
const CONTROL_ID = "spinal-phase0b-control";
const INBOUND_ATTRIBUTE = "data-spinal-phase0b-inbound";
const OUTBOUND_ATTRIBUTE = "data-spinal-phase0b-outbound";
const OBSERVATION_ID = "spinal-phase0b-observation";
const COMPLETE_ATTRIBUTE = "data-spinal-phase0b-complete";
const STATE_ATTRIBUTE = "data-spinal-phase0b-state";
const MAX_PROTOCOL_BYTES = 64 * 1024;
const MAX_EVENT_WINDOW_BYTES = 256 * 1024;
const MAX_OUTER_TERMINAL_BYTES = 8 * 1024 * 1024
  + 2 * MAX_EVENT_WINDOW_BYTES
  + 64 * 1024;
const MAX_PNG_BYTES = 4 * 1024 * 1024;
const MAX_CDP_FRAME_BYTES = 16 * 1024 * 1024;
const MAX_TARGET_HTTP_BYTES = 256 * 1024;
const MAX_VERSION_HTTP_BYTES = 64 * 1024;
const MAX_TARGET_COUNT = 32;
const MAX_PROVENANCE_BYTES = 64 * 1024;
const MAX_SERVED_FILES = 256;
const MAX_SERVED_DIRECTORIES = 256;
const MAX_SERVED_BYTES = 128 * 1024 * 1024;
const MAX_TOOL_OUTPUT_BYTES = 64 * 1024;
const MAX_SHORT_STRING_BYTES = 256;
const MAX_GPU_STRING_BYTES = 1024;
const MAX_GPU_ENTRIES = 128;
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
const GIT_OBJECT_ID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const SEMVER = /^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$/;
const SAFE_RELATIVE_COMPONENT = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
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
const PROVENANCE_FILE = "phase0b-browser-provenance-receipt.json";
const FAILURE_FILE = "phase0b-browser-capture-failure.json";
const DRIVER_PATH = fileURLToPath(import.meta.url);
const REPOSITORY_ROOT = path.dirname(path.dirname(DRIVER_PATH));
const NULL_DEVICE = typeof os.devNull === "string"
  ? os.devNull
  : (process.platform === "win32" ? "\\\\.\\NUL" : "/dev/null");

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

const EVENT_WINDOW_KEYS = Object.freeze([
  "format_version", "window_id", "animation", "start_ns", "end_ns", "events",
]);
const EVENT_KEYS = Object.freeze([
  "animation", "name", "local_time_ns", "loop_index", "integer", "float", "string",
  "volume", "balance", "diagnostic_codes",
]);
const FIXED_EVENT_VECTOR = Object.freeze([
  Object.freeze({
    name: "start", local_time_ns: 0, float: 0, string: null, volume: 1, balance: 0,
  }),
  Object.freeze({
    name: "middle", local_time_ns: 500_000_000,
    float: 1.25, string: "middle", volume: 1, balance: 0,
  }),
  Object.freeze({
    name: "end", local_time_ns: 1_000_000_000,
    float: 0, string: null, volume: 0.5, balance: -0.25,
  }),
]);

function requireRawInteger(rawFields, field, expected, context) {
  if (rawFields.get(field) !== String(expected)) {
    fail(`${context}.${field} is not the required canonical integer`);
  }
}

function validateFixtureEvent(value, raw, source, index, integer) {
  const context = `event_windows.${source}.events[${index}]`;
  exactKeys(value, EVENT_KEYS, context);
  const compact = validateCompactJson(raw, context);
  const expected = FIXED_EVENT_VECTOR[index];
  if (value.animation !== "sway" || value.name !== expected.name
    || value.local_time_ns !== expected.local_time_ns || value.loop_index !== 0
    || value.integer !== integer
    || !Object.is(value.float, expected.float)
    || value.string !== expected.string
    || !Object.is(value.volume, expected.volume)
    || !Object.is(value.balance, expected.balance)) {
    fail(`${context} does not match the fixed self-authored fixture vector`);
  }
  for (const [field, expectedInteger] of [
    ["local_time_ns", expected.local_time_ns],
    ["loop_index", 0],
    ["integer", integer],
  ]) {
    requireRawInteger(compact.topLevelValues, field, expectedInteger, context);
  }
  if (!Array.isArray(value.diagnostic_codes) || value.diagnostic_codes.length !== 0) {
    fail(`${context}.diagnostic_codes must be empty`);
  }
}

function validateEventWindow(value, raw, source, integerBase) {
  const context = `event_windows.${source}`;
  if (typeof raw !== "string" || Buffer.byteLength(raw, "utf8") > MAX_EVENT_WINDOW_BYTES) {
    fail(`${context} is missing or too large`);
  }
  exactKeys(value, EVENT_WINDOW_KEYS, context);
  const compact = validateCompactJson(raw, context);
  if (value.format_version !== 1 || value.window_id !== "sway-events"
    || value.animation !== "sway" || value.start_ns !== 0
    || value.end_ns !== 1_000_000_000) {
    fail(`${context} does not match the fixed v1 event window`);
  }
  for (const [field, expected] of [
    ["format_version", 1],
    ["start_ns", 0],
    ["end_ns", 1_000_000_000],
  ]) {
    requireRawInteger(compact.topLevelValues, field, expected, context);
  }
  if (!Array.isArray(value.events) || value.events.length !== FIXED_EVENT_VECTOR.length) {
    fail(`${context}.events must contain exactly the three fixed fixture events`);
  }
  const rawEvents = compact.topLevelArrays.get("events");
  if (!rawEvents || rawEvents.length !== FIXED_EVENT_VECTOR.length) {
    fail(`${context}.events has invalid raw array framing`);
  }
  for (let index = 0; index < FIXED_EVENT_VECTOR.length; index += 1) {
    validateFixtureEvent(
      value.events[index],
      rawEvents[index],
      source,
      index,
      integerBase + index,
    );
  }
}

function validateEventWindows(value, raw) {
  const context = "event_windows";
  exactKeys(value, ["current", "proposed"], context);
  if (typeof raw !== "string") fail("outer terminal document is missing event_windows");
  const compact = validateCompactJson(raw, context);
  validateEventWindow(value.current, compact.topLevelValues.get("current"), "current", 10);
  validateEventWindow(value.proposed, compact.topLevelValues.get("proposed"), "proposed", 20);
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
    ["format_version", "state", "browser_capture", "event_windows", "observations"],
    "outer terminal document",
  );
  if (value.format_version !== 3 || value.state !== "complete") {
    fail("outer terminal document has the wrong schema or state");
  }
  if (topLevelValues.get("format_version") !== "3") {
    fail("outer terminal format_version is not a canonical integer");
  }
  const nestedRaw = topLevelValues.get("browser_capture");
  if (typeof nestedRaw !== "string") fail("outer terminal document is missing browser_capture");
  parseTerminal(nestedRaw, nonce, runtimeSources, localReceipts);
  validateEventWindows(value.event_windows, topLevelValues.get("event_windows"));
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

function requireBoolean(value, context) {
  if (typeof value !== "boolean") fail(`${context} must be a boolean`);
  return value;
}

function requireInteger(value, minimum, maximum, context) {
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    fail(`${context} must be an integer in ${minimum}..=${maximum}`);
  }
  return value;
}

function requireBoundedString(value, maximumBytes, context, allowEmpty = false) {
  if (typeof value !== "string" || (!allowEmpty && value.length === 0)) {
    fail(`${context} must be ${allowEmpty ? "a" : "a non-empty"} string`);
  }
  if (Buffer.byteLength(value, "utf8") > maximumBytes) {
    fail(`${context} exceeds its ${maximumBytes}-byte budget`);
  }
  if (/\p{Cc}/u.test(value)) fail(`${context} contains a control character`);
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) fail(`${context} contains an unpaired surrogate`);
      index += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      fail(`${context} contains an unpaired surrogate`);
    }
  }
  return value;
}

function compareUtf8(left, right) {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function requireDescriptor(value, expectedFile, maximumBytes, context) {
  exactKeys(value, ["file", "byte_length", "sha256"], context);
  if (value.file !== expectedFile) fail(`${context}.file must be ${expectedFile}`);
  return {
    file: expectedFile,
    byte_length: requireInteger(value.byte_length, 1, maximumBytes, `${context}.byte_length`),
    sha256: requireHex(value.sha256, `${context}.sha256`),
  };
}

function requireByteIdentity(value, maximumBytes, context) {
  exactKeys(value, ["byte_length", "sha256"], context);
  return {
    byte_length: requireInteger(value.byte_length, 1, maximumBytes, `${context}.byte_length`),
    sha256: requireHex(value.sha256, `${context}.sha256`),
  };
}

function requireSemver(value, context) {
  const version = requireBoundedString(value, 128, context);
  if (!SEMVER.test(version)) fail(`${context} must be a parsed semantic version`);
  return version;
}

function requireSafeRelativePath(value, context) {
  const relative = requireBoundedString(value, 240, context);
  const components = relative.split("/");
  if (components.some((component) => !SAFE_RELATIVE_COMPONENT.test(component))) {
    fail(`${context} must be a portable safe relative path`);
  }
  return relative;
}

function canonicalizeProvenanceReceipt(value) {
  exactKeys(value, [
    "format_version", "artifact_kind", "evidence_class", "gate_eligible",
    "relationship", "binding", "build", "browser", "graphics",
  ], "provenance receipt");
  if (value.format_version !== 1
    || value.artifact_kind !== "phase0b_browser_provenance_receipt"
    || value.evidence_class !== "non_representative_rehearsal"
    || value.gate_eligible !== false
    || value.relationship !== "self_reported_context_not_binary_attestation") {
    fail("provenance receipt has an invalid fixed classification");
  }

  exactKeys(
    value.binding,
    ["nonce", "runtime_sources", "capture_manifest", "terminal"],
    "provenance receipt.binding",
  );
  const binding = {
    nonce: requireHex(value.binding.nonce, "provenance receipt.binding.nonce"),
    runtime_sources: validateRuntimeSources(
      value.binding.runtime_sources,
      "provenance receipt.binding.runtime_sources",
    ),
    capture_manifest: requireDescriptor(
      value.binding.capture_manifest,
      MANIFEST_FILE,
      MAX_PROTOCOL_BYTES,
      "provenance receipt.binding.capture_manifest",
    ),
    terminal: requireDescriptor(
      value.binding.terminal,
      TERMINAL_FILE,
      MAX_OUTER_TERMINAL_BYTES,
      "provenance receipt.binding.terminal",
    ),
  };

  exactKeys(value.build, [
    "checkout", "cargo_lock", "trunk_config", "driver", "driver_host",
    "toolchain", "invocation", "served_files",
  ], "provenance receipt.build");
  exactKeys(value.build.checkout, ["head", "dirty", "status_sha256"], "provenance receipt.build.checkout");
  const head = requireBoundedString(value.build.checkout.head, 64, "provenance receipt.build.checkout.head");
  if (!GIT_OBJECT_ID.test(head)) fail("provenance receipt.build.checkout.head is not a Git object ID");
  const checkout = {
    head,
    dirty: requireBoolean(value.build.checkout.dirty, "provenance receipt.build.checkout.dirty"),
    status_sha256: requireHex(
      value.build.checkout.status_sha256,
      "provenance receipt.build.checkout.status_sha256",
    ),
  };
  if (checkout.dirty === (checkout.status_sha256 === sha256(Buffer.alloc(0)))) {
    fail("provenance receipt.build.checkout.dirty does not match the status digest");
  }
  exactKeys(
    value.build.driver_host,
    ["platform", "architecture", "node_version"],
    "provenance receipt.build.driver_host",
  );
  const driverHost = {
    platform: requireBoundedString(value.build.driver_host.platform, 64, "provenance receipt.build.driver_host.platform"),
    architecture: requireBoundedString(value.build.driver_host.architecture, 64, "provenance receipt.build.driver_host.architecture"),
    node_version: requireSemver(value.build.driver_host.node_version, "provenance receipt.build.driver_host.node_version"),
  };
  exactKeys(value.build.toolchain, [
    "rustc_release", "rustc_commit_hash", "rustc_host", "cargo_version",
    "trunk_version", "bevy_version",
  ], "provenance receipt.build.toolchain");
  let rustcCommitHash = value.build.toolchain.rustc_commit_hash;
  if (rustcCommitHash !== null) {
    rustcCommitHash = requireBoundedString(rustcCommitHash, 64, "provenance receipt.build.toolchain.rustc_commit_hash");
    if (!GIT_OBJECT_ID.test(rustcCommitHash)) {
      fail("provenance receipt.build.toolchain.rustc_commit_hash is not a Git object ID or null");
    }
  }
  const bevyVersion = requireSemver(
    value.build.toolchain.bevy_version,
    "provenance receipt.build.toolchain.bevy_version",
  );
  if (bevyVersion !== "0.19.0") {
    fail("provenance receipt.build.toolchain.bevy_version must be the frozen 0.19.0");
  }
  const toolchain = {
    rustc_release: requireSemver(value.build.toolchain.rustc_release, "provenance receipt.build.toolchain.rustc_release"),
    rustc_commit_hash: rustcCommitHash,
    rustc_host: requireBoundedString(value.build.toolchain.rustc_host, 128, "provenance receipt.build.toolchain.rustc_host"),
    cargo_version: requireSemver(value.build.toolchain.cargo_version, "provenance receipt.build.toolchain.cargo_version"),
    trunk_version: requireSemver(value.build.toolchain.trunk_version, "provenance receipt.build.toolchain.trunk_version"),
    bevy_version: bevyVersion,
  };
  exactKeys(
    value.build.invocation,
    ["trunk_release", "target", "features"],
    "provenance receipt.build.invocation",
  );
  if (value.build.invocation.trunk_release !== true
    || value.build.invocation.target !== "wasm32-unknown-unknown"
    || !Array.isArray(value.build.invocation.features)
    || value.build.invocation.features.length !== 1
    || value.build.invocation.features[0] !== "phase0b-rehearsal") {
    fail("provenance receipt.build.invocation is not the fixed rehearsal invocation");
  }
  if (!Array.isArray(value.build.served_files)
    || value.build.served_files.length < 1
    || value.build.served_files.length > MAX_SERVED_FILES) {
    fail(`provenance receipt.build.served_files must contain 1..=${MAX_SERVED_FILES} files`);
  }
  let previousPath;
  let servedTotal = 0;
  const servedFiles = value.build.served_files.map((item, index) => {
    const context = `provenance receipt.build.served_files[${index}]`;
    exactKeys(item, ["path", "byte_length", "sha256"], context);
    const filePath = requireSafeRelativePath(item.path, `${context}.path`);
    if (previousPath !== undefined && compareUtf8(previousPath, filePath) >= 0) {
      fail("provenance receipt.build.served_files must be strictly UTF-8-byte sorted and unique");
    }
    previousPath = filePath;
    const byteLength = requireInteger(item.byte_length, 0, MAX_SERVED_BYTES, `${context}.byte_length`);
    servedTotal += byteLength;
    if (servedTotal > MAX_SERVED_BYTES) fail("provenance receipt.build.served_files exceeds its total byte budget");
    return { path: filePath, byte_length: byteLength, sha256: requireHex(item.sha256, `${context}.sha256`) };
  });
  if (servedTotal === 0) fail("provenance receipt.build.served_files must contain non-empty aggregate content");
  const build = {
    checkout,
    cargo_lock: requireByteIdentity(value.build.cargo_lock, 4 * 1024 * 1024, "provenance receipt.build.cargo_lock"),
    trunk_config: requireByteIdentity(value.build.trunk_config, 64 * 1024, "provenance receipt.build.trunk_config"),
    driver: requireByteIdentity(value.build.driver, 4 * 1024 * 1024, "provenance receipt.build.driver"),
    driver_host: driverHost,
    toolchain,
    invocation: {
      trunk_release: true,
      target: "wasm32-unknown-unknown",
      features: ["phase0b-rehearsal"],
    },
    served_files: servedFiles,
  };

  exactKeys(value.browser, [
    "protocol_version", "product", "revision", "js_version", "requested_launch",
  ], "provenance receipt.browser");
  exactKeys(value.browser.requested_launch, [
    "headless", "gl", "angle_backend", "width_px", "height_px", "device_scale_factor",
  ], "provenance receipt.browser.requested_launch");
  const requestedLaunch = value.browser.requested_launch;
  if (requestedLaunch.headless !== "new" || requestedLaunch.gl !== "angle"
    || requestedLaunch.angle_backend !== "swiftshader"
    || requestedLaunch.width_px !== WIDTH || requestedLaunch.height_px !== HEIGHT
    || requestedLaunch.device_scale_factor !== 1) {
    fail("provenance receipt.browser.requested_launch is not the fixed rehearsal launch");
  }
  const browser = {
    protocol_version: requireBoundedString(value.browser.protocol_version, MAX_SHORT_STRING_BYTES, "provenance receipt.browser.protocol_version"),
    product: requireBoundedString(value.browser.product, MAX_SHORT_STRING_BYTES, "provenance receipt.browser.product"),
    revision: requireBoundedString(value.browser.revision, MAX_SHORT_STRING_BYTES, "provenance receipt.browser.revision"),
    js_version: requireBoundedString(value.browser.js_version, MAX_SHORT_STRING_BYTES, "provenance receipt.browser.js_version"),
    requested_launch: {
      headless: "new",
      gl: "angle",
      angle_backend: "swiftshader",
      width_px: WIDTH,
      height_px: HEIGHT,
      device_scale_factor: 1,
    },
  };

  exactKeys(value.graphics, [
    "system_devices", "feature_status", "driver_bug_workarounds", "effective_context",
  ], "provenance receipt.graphics");
  if (!Array.isArray(value.graphics.system_devices)
    || value.graphics.system_devices.length < 1 || value.graphics.system_devices.length > 8) {
    fail("provenance receipt.graphics.system_devices must contain 1..=8 devices");
  }
  const systemDevices = value.graphics.system_devices.map((device, index) => {
    const context = `provenance receipt.graphics.system_devices[${index}]`;
    exactKeys(device, [
      "vendor_id", "device_id", "vendor_string", "device_string", "driver_vendor",
      "driver_version",
    ], context);
    return {
      vendor_id: requireInteger(device.vendor_id, 0, 0xffff_ffff, `${context}.vendor_id`),
      device_id: requireInteger(device.device_id, 0, 0xffff_ffff, `${context}.device_id`),
      vendor_string: requireBoundedString(device.vendor_string, MAX_GPU_STRING_BYTES, `${context}.vendor_string`, true),
      device_string: requireBoundedString(device.device_string, MAX_GPU_STRING_BYTES, `${context}.device_string`, true),
      driver_vendor: requireBoundedString(device.driver_vendor, MAX_GPU_STRING_BYTES, `${context}.driver_vendor`, true),
      driver_version: requireBoundedString(device.driver_version, MAX_GPU_STRING_BYTES, `${context}.driver_version`, true),
    };
  });
  if (!Array.isArray(value.graphics.feature_status)
    || value.graphics.feature_status.length < 1
    || value.graphics.feature_status.length > MAX_GPU_ENTRIES) {
    fail(`provenance receipt.graphics.feature_status must contain 1..=${MAX_GPU_ENTRIES} entries`);
  }
  let previousFeature;
  const featureStatus = value.graphics.feature_status.map((entry, index) => {
    const context = `provenance receipt.graphics.feature_status[${index}]`;
    exactKeys(entry, ["name", "status"], context);
    const name = requireBoundedString(entry.name, MAX_SHORT_STRING_BYTES, `${context}.name`);
    if (previousFeature !== undefined && compareUtf8(previousFeature, name) >= 0) {
      fail("provenance receipt.graphics.feature_status must be strictly UTF-8-byte sorted and unique");
    }
    previousFeature = name;
    return { name, status: requireBoundedString(entry.status, MAX_SHORT_STRING_BYTES, `${context}.status`) };
  });
  if (!Array.isArray(value.graphics.driver_bug_workarounds)
    || value.graphics.driver_bug_workarounds.length > MAX_GPU_ENTRIES) {
    fail(`provenance receipt.graphics.driver_bug_workarounds exceeds ${MAX_GPU_ENTRIES} entries`);
  }
  let previousWorkaround;
  const workarounds = value.graphics.driver_bug_workarounds.map((item, index) => {
    const workaround = requireBoundedString(
      item,
      MAX_SHORT_STRING_BYTES,
      `provenance receipt.graphics.driver_bug_workarounds[${index}]`,
    );
    if (previousWorkaround !== undefined && compareUtf8(previousWorkaround, workaround) >= 0) {
      fail("provenance receipt.graphics.driver_bug_workarounds must be strictly UTF-8-byte sorted and unique");
    }
    previousWorkaround = workaround;
    return workaround;
  });
  exactKeys(value.graphics.effective_context, [
    "api", "drawing_buffer_width", "drawing_buffer_height", "vendor", "renderer",
    "version", "shading_language_version", "unmasked_vendor", "unmasked_renderer",
  ], "provenance receipt.graphics.effective_context");
  const context = value.graphics.effective_context;
  if (context.api !== "webgl2" || context.drawing_buffer_width !== WIDTH
    || context.drawing_buffer_height !== HEIGHT) {
    fail("provenance receipt.graphics.effective_context is not the required 640x480 WebGL2 context");
  }
  const effectiveContext = {
    api: "webgl2",
    drawing_buffer_width: WIDTH,
    drawing_buffer_height: HEIGHT,
    vendor: requireBoundedString(context.vendor, MAX_GPU_STRING_BYTES, "provenance receipt.graphics.effective_context.vendor"),
    renderer: requireBoundedString(context.renderer, MAX_GPU_STRING_BYTES, "provenance receipt.graphics.effective_context.renderer"),
    version: requireBoundedString(context.version, MAX_GPU_STRING_BYTES, "provenance receipt.graphics.effective_context.version"),
    shading_language_version: requireBoundedString(context.shading_language_version, MAX_GPU_STRING_BYTES, "provenance receipt.graphics.effective_context.shading_language_version"),
    unmasked_vendor: requireBoundedString(context.unmasked_vendor, MAX_GPU_STRING_BYTES, "provenance receipt.graphics.effective_context.unmasked_vendor"),
    unmasked_renderer: requireBoundedString(context.unmasked_renderer, MAX_GPU_STRING_BYTES, "provenance receipt.graphics.effective_context.unmasked_renderer"),
  };
  return {
    format_version: 1,
    artifact_kind: "phase0b_browser_provenance_receipt",
    evidence_class: "non_representative_rehearsal",
    gate_eligible: false,
    relationship: "self_reported_context_not_binary_attestation",
    binding,
    build,
    browser,
    graphics: {
      system_devices: systemDevices,
      feature_status: featureStatus,
      driver_bug_workarounds: workarounds,
      effective_context: effectiveContext,
    },
  };
}

function provenanceReceiptJson(value) {
  const raw = JSON.stringify(canonicalizeProvenanceReceipt(value));
  const byteLength = Buffer.byteLength(raw, "utf8");
  if (byteLength < 1 || byteLength > MAX_PROVENANCE_BYTES) {
    fail(`provenance receipt exceeds its ${MAX_PROVENANCE_BYTES}-byte budget`);
  }
  return raw;
}

function sameFileIdentity(left, right) {
  return left.dev === right.dev && left.ino === right.ino && left.mode === right.mode
    && left.uid === right.uid && left.gid === right.gid
    && left.nlink === right.nlink && left.size === right.size
    && left.mtimeNs === right.mtimeNs && left.ctimeNs === right.ctimeNs;
}

function privateFileIdentity(relative, kind, stat) {
  return {
    path: relative,
    kind,
    dev: stat.dev.toString(),
    ino: stat.ino.toString(),
    mode: stat.mode.toString(),
    uid: stat.uid.toString(),
    gid: stat.gid.toString(),
    nlink: stat.nlink.toString(),
    size: stat.size.toString(),
    mtime_ns: stat.mtimeNs.toString(),
    ctime_ns: stat.ctimeNs.toString(),
  };
}

function readStableRegularFile(file, maximumBytes, context) {
  let initial;
  try {
    if (fs.realpathSync.native(file) !== file) fail(`${context} uses a symlink alias`);
    initial = fs.lstatSync(file, { bigint: true });
  } catch (_error) {
    fail(`${context} is missing or unreadable`);
  }
  if (!initial.isFile() || initial.isSymbolicLink() || initial.nlink !== 1n
    || initial.size < 0n || initial.size > BigInt(maximumBytes)) {
    fail(`${context} must be a single-link regular file within its byte budget`);
  }
  const flags = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0);
  let descriptor;
  try {
    descriptor = fs.openSync(file, flags);
    const opened = fs.fstatSync(descriptor, { bigint: true });
    if (!sameFileIdentity(initial, opened)) fail(`${context} identity changed while opening`);
    const byteLength = Number(opened.size);
    const bytes = Buffer.alloc(byteLength);
    let offset = 0;
    while (offset < byteLength) {
      const count = fs.readSync(descriptor, bytes, offset, byteLength - offset, offset);
      if (count === 0) fail(`${context} ended before its declared length`);
      offset += count;
    }
    const finished = fs.fstatSync(descriptor, { bigint: true });
    if (!sameFileIdentity(opened, finished)) fail(`${context} identity changed while reading`);
    const finalPath = fs.lstatSync(file, { bigint: true });
    if (!sameFileIdentity(finished, finalPath)) fail(`${context} path identity changed while reading`);
    return bytes;
  } finally {
    if (descriptor !== undefined) fs.closeSync(descriptor);
  }
}

function byteIdentity(bytes) {
  return { byte_length: bytes.length, sha256: sha256(bytes) };
}

function secureServedInventory(root) {
  const rootStat = fs.lstatSync(root, { bigint: true });
  if (!rootStat.isDirectory() || rootStat.isSymbolicLink()
    || fs.realpathSync.native(root) !== root) {
    fail("SERVED_ROOT must remain a canonical directory without symlink aliases");
  }
  const pending = [{ absolute: root, relative: "" }];
  const files = [];
  const identities = [];
  let directoryCount = 0;
  let totalBytes = 0;
  while (pending.length > 0) {
    const directory = pending.pop();
    directoryCount += 1;
    if (directoryCount > MAX_SERVED_DIRECTORIES) {
      fail(`SERVED_ROOT exceeds ${MAX_SERVED_DIRECTORIES} directories`);
    }
    const directoryBefore = fs.lstatSync(directory.absolute, { bigint: true });
    if (!directoryBefore.isDirectory() || directoryBefore.isSymbolicLink()) {
      fail("SERVED_ROOT contains a non-directory or symlink where a directory was expected");
    }
    const names = fs.readdirSync(directory.absolute);
    names.sort(compareUtf8);
    for (let index = names.length - 1; index >= 0; index -= 1) {
      const name = names[index];
      if (!SAFE_RELATIVE_COMPONENT.test(name)) {
        fail("SERVED_ROOT contains a non-portable path component");
      }
      const relative = directory.relative === "" ? name : `${directory.relative}/${name}`;
      requireSafeRelativePath(relative, "served file path");
      const absolute = path.join(directory.absolute, name);
      const stat = fs.lstatSync(absolute, { bigint: true });
      if (stat.isSymbolicLink()) fail("SERVED_ROOT contains a symbolic link");
      if (stat.isDirectory()) {
        pending.push({ absolute, relative });
      } else if (stat.isFile()) {
        if (stat.nlink !== 1n) fail("SERVED_ROOT contains a hard-linked file");
        if (files.length >= MAX_SERVED_FILES) {
          fail(`SERVED_ROOT exceeds ${MAX_SERVED_FILES} files`);
        }
        if (stat.size < 0n || stat.size > BigInt(MAX_SERVED_BYTES - totalBytes)) {
          fail(`SERVED_ROOT exceeds ${MAX_SERVED_BYTES} total bytes`);
        }
        const bytes = readStableRegularFile(
          absolute,
          MAX_SERVED_BYTES - totalBytes,
          `served file ${relative}`,
        );
        const finalStat = fs.lstatSync(absolute, { bigint: true });
        if (!sameFileIdentity(stat, finalStat)) {
          fail(`served file ${relative} identity changed during inventory`);
        }
        totalBytes += bytes.length;
        files.push({ path: relative, byte_length: bytes.length, sha256: sha256(bytes) });
        identities.push(privateFileIdentity(relative, "file", finalStat));
      } else {
        fail("SERVED_ROOT contains a special file");
      }
    }
    const directoryAfter = fs.lstatSync(directory.absolute, { bigint: true });
    if (!sameFileIdentity(directoryBefore, directoryAfter)) {
      fail("SERVED_ROOT directory identity changed during inventory");
    }
    identities.push(privateFileIdentity(
      directory.relative === "" ? "." : directory.relative,
      "directory",
      directoryAfter,
    ));
  }
  if (files.length === 0) fail("SERVED_ROOT must contain at least one regular file");
  files.sort((left, right) => compareUtf8(left.path, right.path));
  identities.sort((left, right) => compareUtf8(left.path, right.path));
  return { files, identities };
}

function inventoriesEqual(left, right) {
  return left.files.length === right.files.length
    && left.files.every((item, index) => (
      item.path === right.files[index].path
        && item.byte_length === right.files[index].byte_length
        && item.sha256 === right.files[index].sha256
    ))
    && JSON.stringify(left.identities) === JSON.stringify(right.identities);
}

function sanitizedCommandEnvironment(baseEnvironment = process.env) {
  const environment = Object.create(null);
  for (const [key, value] of Object.entries(baseEnvironment)) {
    if (!key.toUpperCase().startsWith("GIT_") && value !== undefined) {
      environment[key] = value;
    }
  }
  environment.LC_ALL = "C";
  environment.LANG = "C";
  environment.NO_COLOR = "1";
  environment.GIT_CONFIG_NOSYSTEM = "1";
  environment.GIT_CONFIG_GLOBAL = NULL_DEVICE;
  return environment;
}

function fixedGitArguments(args) {
  return [
    "--no-optional-locks",
    "-c", "core.fsmonitor=false",
    "-c", `core.excludesFile=${NULL_DEVICE}`,
    ...args,
  ];
}

function boundedCommand(executable, args, context, allowEmpty = false) {
  let output;
  try {
    output = execFileSync(executable, args, {
      cwd: REPOSITORY_ROOT,
      encoding: null,
      maxBuffer: MAX_TOOL_OUTPUT_BYTES,
      stdio: ["ignore", "pipe", "pipe"],
      env: sanitizedCommandEnvironment(),
    });
  } catch (_error) {
    fail(`${context} command failed or exceeded its output budget`);
  }
  if (!Buffer.isBuffer(output) || (!allowEmpty && output.length < 1)
    || output.length > MAX_TOOL_OUTPUT_BYTES) {
    fail(`${context} command returned invalid output`);
  }
  return output;
}

function validateRepositoryTopLevel(output) {
  const topLevel = singleLine(output, "Git repository top-level");
  if (!path.isAbsolute(topLevel) || path.resolve(topLevel) !== topLevel) {
    fail("Git repository top-level must be an absolute normalized path");
  }
  let canonical;
  try {
    canonical = fs.realpathSync.native(topLevel);
  } catch (_error) {
    fail("Git repository top-level is missing or inaccessible");
  }
  if (topLevel !== canonical || canonical !== REPOSITORY_ROOT) {
    fail("Git repository top-level does not match the canonical capture-driver checkout");
  }
  return topLevel;
}

function singleLine(output, context) {
  const value = output.toString("utf8").trimEnd();
  if (value.includes("\n") || value.includes("\r")) fail(`${context} must be one line`);
  return requireBoundedString(value, MAX_SHORT_STRING_BYTES, context);
}

function parsedCommandVersion(output, prefix, context) {
  const line = singleLine(output, context);
  const match = new RegExp(`^${prefix} ([^ ]+)(?: \\([^\\r\\n]+\\))?$`).exec(line);
  if (!match || !SEMVER.test(match[1])) fail(`${context} has an unsupported version format`);
  return match[1];
}

function parseRustcVerbose(output) {
  const raw = output.toString("utf8");
  if (Buffer.byteLength(raw, "utf8") > MAX_TOOL_OUTPUT_BYTES) fail("rustc -vV output is too large");
  const fields = new Map();
  for (const line of raw.split(/\r?\n/)) {
    const pivot = line.indexOf(": ");
    if (pivot < 1) continue;
    const key = line.slice(0, pivot);
    if (fields.has(key)) fail("rustc -vV contains a duplicate field");
    fields.set(key, line.slice(pivot + 2));
  }
  const release = fields.get("release");
  const host = fields.get("host");
  const commit = fields.get("commit-hash");
  if (!SEMVER.test(release || "")) fail("rustc -vV release is not a semantic version");
  requireBoundedString(host, 128, "rustc host");
  let commitHash = commit;
  if (commit === "unknown") commitHash = null;
  else if (!GIT_OBJECT_ID.test(commit || "")) fail("rustc commit hash is invalid");
  return { rustc_release: release, rustc_commit_hash: commitHash, rustc_host: host };
}

function parseBevyVersion(cargoLockBytes) {
  const text = cargoLockBytes.toString("utf8");
  const matches = [];
  for (const block of text.split(/\n(?=\[\[package\]\]\r?\n)/)) {
    if (!/^name = "bevy"$/m.test(block)) continue;
    const version = /^version = "([^"]+)"$/m.exec(block)?.[1];
    if (!version || !SEMVER.test(version)) fail("Cargo.lock has an invalid bevy version");
    matches.push(version);
  }
  if (matches.length !== 1) fail("Cargo.lock must contain exactly one bevy package");
  return matches[0];
}

function collectBuildContext(servedFiles) {
  if (fs.realpathSync.native(DRIVER_PATH) !== DRIVER_PATH
    || fs.realpathSync.native(REPOSITORY_ROOT) !== REPOSITORY_ROOT) {
    fail("capture driver and repository root must not use symlink aliases");
  }
  const cargoLockBytes = readStableRegularFile(
    path.join(REPOSITORY_ROOT, "Cargo.lock"),
    4 * 1024 * 1024,
    "Cargo.lock",
  );
  const trunkConfigBytes = readStableRegularFile(
    path.join(REPOSITORY_ROOT, "apps/spinal/web/Trunk.toml"),
    64 * 1024,
    "Trunk.toml",
  );
  const driverBytes = readStableRegularFile(DRIVER_PATH, 4 * 1024 * 1024, "capture driver");
  validateRepositoryTopLevel(boundedCommand(
    "git",
    fixedGitArguments(["rev-parse", "--show-toplevel"]),
    "git rev-parse --show-toplevel",
  ));
  const head = singleLine(
    boundedCommand(
      "git",
      fixedGitArguments(["rev-parse", "--verify", "HEAD"]),
      "git rev-parse HEAD",
    ),
    "Git HEAD",
  );
  if (!GIT_OBJECT_ID.test(head)) fail("Git HEAD is not a supported object ID");
  const status = boundedCommand(
    "git",
    fixedGitArguments([
      "status", "--porcelain=v1", "-z", "--untracked-files=all",
      "--ignore-submodules=none",
    ]),
    "git status",
    true,
  );
  const rustc = parseRustcVerbose(boundedCommand("rustc", ["-vV"], "rustc -vV"));
  return {
    checkout: { head, dirty: status.length !== 0, status_sha256: sha256(status) },
    cargo_lock: byteIdentity(cargoLockBytes),
    trunk_config: byteIdentity(trunkConfigBytes),
    driver: byteIdentity(driverBytes),
    driver_host: {
      platform: requireBoundedString(process.platform, 64, "driver platform"),
      architecture: requireBoundedString(process.arch, 64, "driver architecture"),
      node_version: requireSemver(process.versions.node, "Node.js version"),
    },
    toolchain: {
      ...rustc,
      cargo_version: parsedCommandVersion(boundedCommand("cargo", ["--version"], "cargo --version"), "cargo", "cargo version"),
      trunk_version: parsedCommandVersion(boundedCommand("trunk", ["--version"], "trunk --version"), "trunk", "trunk version"),
      bevy_version: parseBevyVersion(cargoLockBytes),
    },
    invocation: {
      trunk_release: true,
      target: "wasm32-unknown-unknown",
      features: ["phase0b-rehearsal"],
    },
    served_files: servedFiles,
  };
}

function parseCli(args) {
  if (args.length === 1 && args[0] === "--self-test") return { mode: "self-test" };
  if (args.length !== 5) {
    fail("usage: node tools/spinal-phase0b-cdp.js PORT EXPECTED_PAGE_URL NEW_OUTPUT_DIR SERVED_ROOT NONCE");
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
  if (!args[3] || args[3].includes("\0") || !path.isAbsolute(args[3])
    || path.resolve(args[3]) !== args[3] || path.parse(args[3]).root === args[3]) {
    fail("SERVED_ROOT must be an absolute normalized non-root path");
  }
  let servedStat;
  let realServedRoot;
  try {
    servedStat = fs.lstatSync(args[3]);
    realServedRoot = fs.realpathSync.native(args[3]);
  } catch (_error) {
    fail("SERVED_ROOT must already exist");
  }
  if (!servedStat.isDirectory() || servedStat.isSymbolicLink() || realServedRoot !== args[3]) {
    fail("SERVED_ROOT must be a canonical directory without symlink aliases");
  }
  if (args[2].startsWith(`${args[3]}${path.sep}`)) {
    fail("NEW_OUTPUT_DIR must not be inside SERVED_ROOT");
  }
  const nonce = requireHex(args[4], "NONCE");
  return {
    mode: "capture",
    port,
    expectedPageUrl: args[1],
    outputDir: args[2],
    servedRoot: args[3],
    nonce,
  };
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function fetchBoundedJson(port, endpoint, maximumBytes, context) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 1_000);
  try {
    const response = await fetch(`http://127.0.0.1:${port}${endpoint}`, { signal: controller.signal });
    if (!response.ok) fail(`${context} returned HTTP ${response.status}`);
    const contentLength = response.headers.get("content-length");
    if (contentLength !== null) {
      if (!/^(?:0|[1-9][0-9]*)$/.test(contentLength)
        || Number(contentLength) > maximumBytes) {
        fail(`${context} declared an invalid or oversized body`);
      }
    }
    if (!response.body) fail(`${context} returned no body`);
    const chunks = [];
    let byteLength = 0;
    const reader = response.body.getReader();
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (!(value instanceof Uint8Array)) fail(`${context} returned an invalid body chunk`);
      byteLength += value.byteLength;
      if (byteLength > maximumBytes) {
        controller.abort();
        fail(`${context} exceeded its ${maximumBytes}-byte budget`);
      }
      chunks.push(Buffer.from(value));
    }
    if (byteLength < 1) fail(`${context} returned an empty body`);
    let raw;
    try {
      raw = new TextDecoder("utf-8", { fatal: true }).decode(Buffer.concat(chunks, byteLength));
    } catch (_error) {
      fail(`${context} returned invalid UTF-8`);
    }
    try {
      return JSON.parse(raw);
    } catch (_error) {
      fail(`${context} returned invalid JSON`);
    }
  } finally {
    clearTimeout(timer);
  }
}

async function fetchTargets(port) {
  const value = await fetchBoundedJson(
    port,
    "/json/list",
    MAX_TARGET_HTTP_BYTES,
    "CDP target endpoint",
  );
  if (!Array.isArray(value) || value.length > MAX_TARGET_COUNT) {
    fail(`CDP target endpoint must return at most ${MAX_TARGET_COUNT} targets`);
  }
  return value;
}

function validateDebuggerUrl(value, port, targetKind) {
  let url;
  try {
    url = new URL(value);
  } catch (_error) {
    fail(`matching CDP ${targetKind} has an invalid debugger URL`);
  }
  if (
    url.protocol !== "ws:"
    || url.hostname !== "127.0.0.1"
    || url.port !== String(port)
    || url.username !== ""
    || url.password !== ""
    || url.hash !== ""
    || url.search !== ""
    || !url.pathname.startsWith(`/devtools/${targetKind}/`)
  ) {
    fail(`matching CDP ${targetKind} debugger URL is not confined to its fixed loopback endpoint`);
  }
  return url.href;
}

async function findBrowserTarget(port) {
  const value = await fetchBoundedJson(
    port,
    "/json/version",
    MAX_VERSION_HTTP_BYTES,
    "CDP version endpoint",
  );
  if (value === null || Array.isArray(value) || typeof value !== "object"
    || typeof value.webSocketDebuggerUrl !== "string") {
    fail("CDP version endpoint did not expose a browser debugger URL");
  }
  return validateDebuggerUrl(value.webSocketDebuggerUrl, port, "browser");
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
        return validateDebuggerUrl(matches[0].webSocketDebuggerUrl, port, "page");
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
      if (typeof event.data !== "string") throw new Error("binary CDP frame");
      const byteLength = Buffer.byteLength(event.data, "utf8");
      if (byteLength < 1 || byteLength > MAX_CDP_FRAME_BYTES) {
        throw new Error("oversized CDP frame");
      }
      message = JSON.parse(event.data);
    } catch (_error) {
      rejectAll(new Error("CDP sent an invalid, binary, or oversized JSON message"));
      closeSocketQuietly(socket);
      return;
    }
    if (message === null || Array.isArray(message) || typeof message !== "object") return;
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

function normalizeBrowserVersion(value) {
  if (value === null || Array.isArray(value) || typeof value !== "object") {
    fail("Browser.getVersion did not return an object");
  }
  return {
    protocol_version: requireBoundedString(value.protocolVersion, MAX_SHORT_STRING_BYTES, "browser protocol version"),
    product: requireBoundedString(value.product, MAX_SHORT_STRING_BYTES, "browser product"),
    revision: requireBoundedString(value.revision, MAX_SHORT_STRING_BYTES, "browser revision"),
    js_version: requireBoundedString(value.jsVersion, MAX_SHORT_STRING_BYTES, "browser JavaScript version"),
    requested_launch: {
      headless: "new",
      gl: "angle",
      angle_backend: "swiftshader",
      width_px: WIDTH,
      height_px: HEIGHT,
      device_scale_factor: 1,
    },
  };
}

function normalizeSystemInfo(value) {
  if (value === null || Array.isArray(value) || typeof value !== "object"
    || value.gpu === null || Array.isArray(value.gpu) || typeof value.gpu !== "object") {
    fail("SystemInfo.getInfo did not return GPU information");
  }
  const { gpu } = value;
  if (!Array.isArray(gpu.devices) || gpu.devices.length < 1 || gpu.devices.length > 8) {
    fail("SystemInfo.getInfo GPU devices must contain 1..=8 entries");
  }
  const systemDevices = gpu.devices.map((device, index) => {
    if (device === null || Array.isArray(device) || typeof device !== "object") {
      fail(`SystemInfo.getInfo GPU device ${index} is invalid`);
    }
    return {
      vendor_id: requireInteger(device.vendorId, 0, 0xffff_ffff, `GPU device ${index} vendorId`),
      device_id: requireInteger(device.deviceId, 0, 0xffff_ffff, `GPU device ${index} deviceId`),
      vendor_string: requireBoundedString(device.vendorString, MAX_GPU_STRING_BYTES, `GPU device ${index} vendorString`, true),
      device_string: requireBoundedString(device.deviceString, MAX_GPU_STRING_BYTES, `GPU device ${index} deviceString`, true),
      driver_vendor: requireBoundedString(device.driverVendor, MAX_GPU_STRING_BYTES, `GPU device ${index} driverVendor`, true),
      driver_version: requireBoundedString(device.driverVersion, MAX_GPU_STRING_BYTES, `GPU device ${index} driverVersion`, true),
    };
  });
  if (gpu.featureStatus === null || Array.isArray(gpu.featureStatus)
    || typeof gpu.featureStatus !== "object") {
    fail("SystemInfo.getInfo GPU featureStatus is invalid");
  }
  const featureEntries = Object.entries(gpu.featureStatus);
  if (featureEntries.length < 1 || featureEntries.length > MAX_GPU_ENTRIES) {
    fail(`SystemInfo.getInfo GPU featureStatus must contain 1..=${MAX_GPU_ENTRIES} entries`);
  }
  const featureStatus = featureEntries.map(([name, status]) => ({
    name: requireBoundedString(name, MAX_SHORT_STRING_BYTES, "GPU feature name"),
    status: requireBoundedString(status, MAX_SHORT_STRING_BYTES, `GPU feature ${name} status`),
  }));
  featureStatus.sort((left, right) => compareUtf8(left.name, right.name));
  if (!Array.isArray(gpu.driverBugWorkarounds)
    || gpu.driverBugWorkarounds.length > MAX_GPU_ENTRIES) {
    fail(`SystemInfo.getInfo GPU driverBugWorkarounds exceeds ${MAX_GPU_ENTRIES} entries`);
  }
  const workaroundSet = new Set();
  for (const [index, item] of gpu.driverBugWorkarounds.entries()) {
    workaroundSet.add(requireBoundedString(
      item,
      MAX_SHORT_STRING_BYTES,
      `GPU driver workaround ${index}`,
    ));
  }
  const driverBugWorkarounds = [...workaroundSet];
  driverBugWorkarounds.sort(compareUtf8);
  return {
    system_devices: systemDevices,
    feature_status: featureStatus,
    driver_bug_workarounds: driverBugWorkarounds,
  };
}

async function effectiveWebGlContext(command) {
  const expression = `(()=>{
    const canvases=document.querySelectorAll("#spinal-canvas");
    if(canvases.length!==1||!(canvases[0] instanceof HTMLCanvasElement))throw new Error("canvas");
    const gl=canvases[0].getContext("webgl2");
    if(!(gl instanceof WebGL2RenderingContext)||gl.isContextLost())throw new Error("webgl2");
    const debug=gl.getExtension("WEBGL_debug_renderer_info");
    if(!debug)throw new Error("debug renderer");
    const text=(parameter)=>{const value=gl.getParameter(parameter);if(typeof value!=="string"||value.length===0)throw new Error("parameter");return value;};
    return {api:"webgl2",drawing_buffer_width:gl.drawingBufferWidth,drawing_buffer_height:gl.drawingBufferHeight,vendor:text(gl.VENDOR),renderer:text(gl.RENDERER),version:text(gl.VERSION),shading_language_version:text(gl.SHADING_LANGUAGE_VERSION),unmasked_vendor:text(debug.UNMASKED_VENDOR_WEBGL),unmasked_renderer:text(debug.UNMASKED_RENDERER_WEBGL)};
  })()`;
  const response = await command("Runtime.evaluate", {
    expression,
    silent: true,
    returnByValue: true,
    awaitPromise: false,
  });
  if (response.exceptionDetails || response.result?.type !== "object"
    || response.result.subtype !== undefined || response.result.value === undefined) {
    fail("CDP could not inspect the existing #spinal-canvas WebGL2 debug-renderer context");
  }
  const value = response.result.value;
  exactKeys(value, [
    "api", "drawing_buffer_width", "drawing_buffer_height", "vendor", "renderer",
    "version", "shading_language_version", "unmasked_vendor", "unmasked_renderer",
  ], "effective WebGL2 context");
  if (value.api !== "webgl2" || value.drawing_buffer_width !== WIDTH
    || value.drawing_buffer_height !== HEIGHT) {
    fail("existing #spinal-canvas context is not an effective 640x480 WebGL2 context");
  }
  return {
    api: "webgl2",
    drawing_buffer_width: WIDTH,
    drawing_buffer_height: HEIGHT,
    vendor: requireBoundedString(value.vendor, MAX_GPU_STRING_BYTES, "effective WebGL2 vendor"),
    renderer: requireBoundedString(value.renderer, MAX_GPU_STRING_BYTES, "effective WebGL2 renderer"),
    version: requireBoundedString(value.version, MAX_GPU_STRING_BYTES, "effective WebGL2 version"),
    shading_language_version: requireBoundedString(value.shading_language_version, MAX_GPU_STRING_BYTES, "effective WebGL2 shading language version"),
    unmasked_vendor: requireBoundedString(value.unmasked_vendor, MAX_GPU_STRING_BYTES, "effective WebGL2 unmasked vendor"),
    unmasked_renderer: requireBoundedString(value.unmasked_renderer, MAX_GPU_STRING_BYTES, "effective WebGL2 unmasked renderer"),
  };
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
  const nonce = options.nonce;
  let cdp;
  let browserCdp;
  try {
    const servedInventory = secureServedInventory(options.servedRoot);
    const build = collectBuildContext(servedInventory.files);
    const browserDebuggerUrl = await findBrowserTarget(options.port);
    browserCdp = await connectCdp(browserDebuggerUrl);
    const browserBefore = normalizeBrowserVersion(
      await browserCdp.command("Browser.getVersion"),
    );
    const systemBefore = normalizeSystemInfo(
      await browserCdp.command("SystemInfo.getInfo"),
    );
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
    let effectiveContext;
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
      if (sequence === 0) effectiveContext = await effectiveWebGlContext(command);
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
    if (effectiveContext === undefined) fail("effective WebGL2 context was not observed");
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
    const browserAfter = normalizeBrowserVersion(
      await browserCdp.command("Browser.getVersion"),
    );
    const systemAfter = normalizeSystemInfo(
      await browserCdp.command("SystemInfo.getInfo"),
    );
    const effectiveContextAfter = await effectiveWebGlContext(command);
    if (JSON.stringify(browserAfter) !== JSON.stringify(browserBefore)) {
      fail("Browser.getVersion changed during capture");
    }
    if (JSON.stringify(systemAfter) !== JSON.stringify(systemBefore)) {
      fail("SystemInfo.getInfo GPU context changed during capture");
    }
    if (JSON.stringify(effectiveContextAfter) !== JSON.stringify(effectiveContext)) {
      fail("effective #spinal-canvas WebGL2 context changed during capture");
    }
    const servedInventoryAfter = secureServedInventory(options.servedRoot);
    if (!inventoriesEqual(servedInventory, servedInventoryAfter)) {
      fail("SERVED_ROOT file identity or content changed during capture");
    }
    const buildAfter = collectBuildContext(servedInventoryAfter.files);
    if (JSON.stringify(buildAfter) !== JSON.stringify(build)) {
      fail("checkout, build input, driver, host, or toolchain context changed during capture");
    }
    writeCreateOnly(options.outputDir, TERMINAL_FILE, terminalBytes);
    const terminal = { byte_length: terminalBytes.length, sha256: sha256(terminalBytes) };
    const manifest = captureManifestJson(nonce, terminal, files);
    const manifestBytes = Buffer.from(manifest, "utf8");
    writeCreateOnly(options.outputDir, MANIFEST_FILE, manifestBytes);
    const provenance = provenanceReceiptJson({
      format_version: 1,
      artifact_kind: "phase0b_browser_provenance_receipt",
      evidence_class: "non_representative_rehearsal",
      gate_eligible: false,
      relationship: "self_reported_context_not_binary_attestation",
      binding: {
        nonce,
        runtime_sources: challengeAck.runtime_sources,
        capture_manifest: {
          file: MANIFEST_FILE,
          byte_length: manifestBytes.length,
          sha256: sha256(manifestBytes),
        },
        terminal: { file: TERMINAL_FILE, ...terminal },
      },
      build,
      browser: browserBefore,
      graphics: { ...systemBefore, effective_context: effectiveContext },
    });
    writeCreateOnly(options.outputDir, PROVENANCE_FILE, Buffer.from(provenance, "utf8"));
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
    browserCdp?.close();
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
  const canonicalTemporaryRoot = fs.realpathSync.native(os.tmpdir());
  const output = path.join(canonicalTemporaryRoot, `.phase0b-cdp-self-test-${process.pid}`);
  const servedRoot = fs.realpathSync.native(process.cwd());
  assert(parseCli(["--self-test"]).mode === "self-test", "self-test CLI");
  assert(
    parseCli(["9222", "http://127.0.0.1:8080/", output, servedRoot, nonce]).nonce === nonce,
    "capture CLI",
  );
  assertThrows(
    () => parseCli(["022", "http://127.0.0.1/", output, servedRoot, nonce]),
    /canonical decimal/,
    "non-canonical port",
  );
  assertThrows(
    () => parseCli(["22", "http://localhost/", output, servedRoot, nonce]),
    /127\.0\.0\.1/,
    "non-loopback page URL",
  );
  assertThrows(
    () => parseCli(["22", "http://127.0.0.1/", "relative", servedRoot, nonce]),
    /absolute normalized/,
    "relative output",
  );
  assertThrows(
    () => parseCli(["22", "http://127.0.0.1/", output, servedRoot, "A".repeat(64)]),
    /64 lowercase/,
    "non-canonical runner nonce",
  );
  assertThrows(() => parseCli([]), /usage:/, "missing CLI arguments");
  const sanitizedEnvironment = sanitizedCommandEnvironment({
    PATH: "/test/bin",
    HOME: "/test/home",
    LANG: "host-locale",
    GIT_DIR: "/attacker/git-dir",
    git_work_tree: "/attacker/work-tree",
    GiT_Config_Count: "1",
    GIT_CONFIG_KEY_0: "core.fsmonitor",
    GIT_CONFIG_VALUE_0: "attacker",
    GIT_CONFIG_NOSYSTEM: "0",
    GIT_CONFIG_GLOBAL: "/attacker/config",
  });
  assert(sanitizedEnvironment.PATH === "/test/bin", "command environment preserves non-Git PATH");
  assert(sanitizedEnvironment.HOME === "/test/home", "command environment preserves non-Git HOME");
  assert(sanitizedEnvironment.LC_ALL === "C" && sanitizedEnvironment.LANG === "C", "command locale is fixed");
  assert(sanitizedEnvironment.NO_COLOR === "1", "command color is disabled");
  assert(!Object.hasOwn(sanitizedEnvironment, "GIT_DIR"), "command environment removes GIT_DIR");
  assert(!Object.hasOwn(sanitizedEnvironment, "git_work_tree"), "command environment removes case-insensitive git variables");
  assert(!Object.hasOwn(sanitizedEnvironment, "GiT_Config_Count"), "command environment removes injected Git config count");
  assert(!Object.hasOwn(sanitizedEnvironment, "GIT_CONFIG_KEY_0"), "command environment removes injected Git config key");
  assert(!Object.hasOwn(sanitizedEnvironment, "GIT_CONFIG_VALUE_0"), "command environment removes injected Git config value");
  assert(sanitizedEnvironment.GIT_CONFIG_NOSYSTEM === "1", "system Git config is disabled");
  assert(sanitizedEnvironment.GIT_CONFIG_GLOBAL === NULL_DEVICE, "global Git config uses the null device");
  const gitArguments = fixedGitArguments(["status"]);
  assert(
    JSON.stringify(gitArguments) === JSON.stringify([
      "--no-optional-locks",
      "-c", "core.fsmonitor=false",
      "-c", `core.excludesFile=${NULL_DEVICE}`,
      "status",
    ]),
    "Git arguments disable optional locks, filesystem monitoring, and global excludes",
  );
  assert(
    validateRepositoryTopLevel(Buffer.from(`${REPOSITORY_ROOT}\n`)) === REPOSITORY_ROOT,
    "canonical Git repository top-level",
  );
  assertThrows(
    () => validateRepositoryTopLevel(Buffer.from(`${canonicalTemporaryRoot}\n`)),
    /does not match/,
    "wrong Git repository top-level",
  );
  assertThrows(
    () => validateRepositoryTopLevel(Buffer.from(`${REPOSITORY_ROOT}${path.sep}.\n`)),
    /absolute normalized/,
    "non-canonical Git repository top-level",
  );
  assertThrows(() => requireBoundedString("x\u0000y", 10, "control test"), /control/, "control string");
  assertThrows(() => requireBoundedString("\ud800", 10, "surrogate test"), /surrogate/, "lone surrogate");
  assert(
    parsedCommandVersion(Buffer.from("cargo 1.95.0 (abcdef 2026-01-01)\n"), "cargo", "cargo test") === "1.95.0",
    "parsed cargo version",
  );
  assertThrows(
    () => parsedCommandVersion(Buffer.from("cargo 1.95\n"), "cargo", "cargo test"),
    /unsupported version/,
    "malformed cargo version",
  );
  const rustcVersion = parseRustcVerbose(Buffer.from(
    "rustc 1.95.0 (abcdef 2026-01-01)\ncommit-hash: 4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a\nhost: aarch64-test-os\nrelease: 1.95.0\n",
  ));
  assert(rustcVersion.rustc_release === "1.95.0", "parsed rustc release");
  assert(rustcVersion.rustc_host === "aarch64-test-os", "parsed rustc host");
  assert(
    parseBevyVersion(Buffer.from('version = 4\n\n[[package]]\nname = "bevy"\nversion = "0.18.1"\n')) === "0.18.1",
    "parsed bevy lockfile version",
  );
  const normalizedBrowser = normalizeBrowserVersion({
    protocolVersion: "1.3",
    product: "Chrome/140.0.0.0",
    revision: "@abcd",
    userAgent: "intentionally omitted",
    jsVersion: "14.0.0",
  });
  assert(normalizedBrowser.product === "Chrome/140.0.0.0", "normalized browser version");
  assert(!Object.hasOwn(normalizedBrowser, "user_agent"), "raw browser user agent omitted");
  const normalizedSystem = normalizeSystemInfo({
    gpu: {
      devices: [{
        vendorId: 1,
        deviceId: 2,
        vendorString: "Vendor",
        deviceString: "Device",
        driverVendor: "Driver",
        driverVersion: "1",
        subSysId: 99,
        revision: 88,
      }],
      auxAttributes: { processId: 123, glRenderer: "/private/path" },
      featureStatus: { zeta: "enabled", alpha: "disabled" },
      driverBugWorkarounds: ["zeta", "alpha", "alpha"],
    },
    modelName: "intentionally omitted",
    commandLine: "intentionally omitted",
  });
  assert(normalizedSystem.feature_status.map(({ name }) => name).join(",") === "alpha,zeta", "sorted GPU feature status");
  assert(normalizedSystem.driver_bug_workarounds.join(",") === "alpha,zeta", "sorted unique GPU workarounds");
  assert(!Object.hasOwn(normalizedSystem.system_devices[0], "revision"), "unstable GPU device fields omitted");

  const inventoryRoot = fs.mkdtempSync(path.join(canonicalTemporaryRoot, "spinal-phase0b-inventory."));
  try {
    fs.mkdirSync(path.join(inventoryRoot, "assets"), { mode: 0o700 });
    fs.writeFileSync(path.join(inventoryRoot, "index.html"), "index", { mode: 0o600 });
    fs.writeFileSync(path.join(inventoryRoot, "assets", "app.js"), "script", { mode: 0o600 });
    const inventory = secureServedInventory(inventoryRoot);
    assert(inventory.files.map((entry) => entry.path).join(",") === "assets/app.js,index.html", "served inventory ordering");
    assert(inventory.identities.length === 4, "served file and directory identity snapshot");
    assert(inventoriesEqual(inventory, secureServedInventory(inventoryRoot)), "served inventory repeatability");
    const replacement = path.join(inventoryRoot, "replacement.html");
    fs.writeFileSync(replacement, "index", { mode: 0o600 });
    fs.renameSync(replacement, path.join(inventoryRoot, "index.html"));
    assert(
      !inventoriesEqual(inventory, secureServedInventory(inventoryRoot)),
      "served same-byte replacement identity detection",
    );
    fs.linkSync(path.join(inventoryRoot, "index.html"), path.join(inventoryRoot, "alias.html"));
    assertThrows(() => secureServedInventory(inventoryRoot), /hard-linked/, "served hard link rejection");
  } finally {
    fs.rmSync(inventoryRoot, { recursive: true, force: false });
  }
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
  const fixturePayloads = [
    { float: 0, string: null, volume: 1, balance: 0 },
    { float: 1.25, string: "middle", volume: 1, balance: 0 },
    { float: 0, string: null, volume: 0.5, balance: -0.25 },
  ];
  const fixtureEventWindow = (integerBase) => ({
    format_version: 1,
    window_id: "sway-events",
    animation: "sway",
    start_ns: 0,
    end_ns: 1_000_000_000,
    events: FIXED_EVENT_VECTOR.map((expected, index) => ({
      animation: "sway",
      name: expected.name,
      local_time_ns: expected.local_time_ns,
      loop_index: 0,
      integer: integerBase + index,
      ...fixturePayloads[index],
      diagnostic_codes: [],
    })),
  });
  const eventWindows = {
    current: fixtureEventWindow(10),
    proposed: fixtureEventWindow(20),
  };
  const outerRaw = JSON.stringify({
    format_version: 3,
    state: "complete",
    browser_capture: terminalDocument,
    event_windows: eventWindows,
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
    () => parseOuterTerminal(outerRaw.replace('"format_version":3', '"format_version":3.0'), nonce, runtimeSources, receipts),
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
  const parsedOuter = JSON.parse(outerRaw);
  const reorderedOuter = `{"state":"complete","format_version":3,"browser_capture":${terminalRaw},"event_windows":${JSON.stringify(eventWindows)},"observations":${JSON.stringify(parsedOuter.observations)}}`;
  assertThrows(() => parseOuterTerminal(reorderedOuter, nonce, runtimeSources, receipts), /field order/, "outer field order");

  const mutateOuter = (mutate) => {
    const document = JSON.parse(outerRaw);
    mutate(document);
    return JSON.stringify(document);
  };
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      delete document.event_windows;
    }), nonce, runtimeSources, receipts),
    /field order/,
    "missing event_windows",
  );
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      document.event_windows = {
        proposed: document.event_windows.proposed,
        current: document.event_windows.current,
      };
    }), nonce, runtimeSources, receipts),
    /field order/,
    "event_windows field order",
  );
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      document.event_windows.current.extra = true;
    }), nonce, runtimeSources, receipts),
    /field order/,
    "extra event window field",
  );
  const duplicateEventField = outerRaw.replace(
    '"name":"start","local_time_ns":0',
    '"name":"start","name":"start","local_time_ns":0',
  );
  assertThrows(
    () => parseOuterTerminal(duplicateEventField, nonce, runtimeSources, receipts),
    /duplicate keys/,
    "duplicate event field",
  );
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      const event = document.event_windows.current.events[0];
      document.event_windows.current.events[0] = {
        name: event.name,
        animation: event.animation,
        local_time_ns: event.local_time_ns,
        loop_index: event.loop_index,
        integer: event.integer,
        float: event.float,
        string: event.string,
        volume: event.volume,
        balance: event.balance,
        diagnostic_codes: event.diagnostic_codes,
      };
    }), nonce, runtimeSources, receipts),
    /field order/,
    "event field order",
  );
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      delete document.event_windows.proposed.events[2].balance;
    }), nonce, runtimeSources, receipts),
    /field order/,
    "missing event field",
  );
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      document.event_windows.proposed.events[2].extra = true;
    }), nonce, runtimeSources, receipts),
    /field order/,
    "extra event field",
  );
  assertThrows(
    () => parseOuterTerminal(outerRaw.replace(
      '"event_windows":{"current":{"format_version":1',
      '"event_windows":{"current":{"format_version":1.0',
    ), nonce, runtimeSources, receipts),
    /canonical integer/,
    "event window version with a decimal spelling",
  );
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      document.event_windows.current.events[1].local_time_ns = 500_000_001;
    }), nonce, runtimeSources, receipts),
    /fixed self-authored fixture vector/,
    "event time vector",
  );
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      document.event_windows.proposed.events[1].integer = 11;
    }), nonce, runtimeSources, receipts),
    /fixed self-authored fixture vector/,
    "source-bound event integer vector",
  );
  for (const [field, value] of [
    ["float", 1.5],
    ["string", "changed"],
    ["volume", 0.75],
    ["balance", 0.25],
  ]) {
    assertThrows(
      () => parseOuterTerminal(mutateOuter((document) => {
        document.event_windows.current.events[1][field] = value;
      }), nonce, runtimeSources, receipts),
      /fixed self-authored fixture vector/,
      `event payload field ${field}`,
    );
  }
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      document.event_windows.current.events[0].diagnostic_codes = ["unknown_field"];
    }), nonce, runtimeSources, receipts),
    /must be empty/,
    "event diagnostics",
  );
  assertThrows(
    () => parseOuterTerminal(mutateOuter((document) => {
      document.event_windows.current.events[0].string = "x".repeat(MAX_EVENT_WINDOW_BYTES);
    }), nonce, runtimeSources, receipts),
    /too large/,
    "event window size bound",
  );

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
  const emptyDigest = sha256(Buffer.alloc(0));
  const fixedProvenance = {
    format_version: 1,
    artifact_kind: "phase0b_browser_provenance_receipt",
    evidence_class: "non_representative_rehearsal",
    gate_eligible: false,
    relationship: "self_reported_context_not_binary_attestation",
    binding: {
      nonce,
      runtime_sources: runtimeSources,
      capture_manifest: {
        file: MANIFEST_FILE,
        byte_length: 17,
        sha256: "4".repeat(64),
      },
      terminal: {
        file: TERMINAL_FILE,
        byte_length: 19,
        sha256: "5".repeat(64),
      },
    },
    build: {
      checkout: {
        head: "6".repeat(40),
        dirty: false,
        status_sha256: emptyDigest,
      },
      cargo_lock: { byte_length: 23, sha256: "7".repeat(64) },
      trunk_config: { byte_length: 29, sha256: "8".repeat(64) },
      driver: { byte_length: 31, sha256: "9".repeat(64) },
      driver_host: {
        platform: "test-os",
        architecture: "test-arch",
        node_version: "24.1.0",
      },
      toolchain: {
        rustc_release: "1.95.0",
        rustc_commit_hash: null,
        rustc_host: "test-target",
        cargo_version: "1.95.0",
        trunk_version: "0.21.14",
        bevy_version: "0.19.0",
      },
      invocation: {
        trunk_release: true,
        target: "wasm32-unknown-unknown",
        features: ["phase0b-rehearsal"],
      },
      served_files: [
        { path: "assets/app.js", byte_length: 37, sha256: "a".repeat(64) },
        { path: "index.html", byte_length: 41, sha256: "b".repeat(64) },
      ],
    },
    browser: {
      protocol_version: "1.3",
      product: "Chrome/140.0.0.0",
      revision: "@test-revision",
      js_version: "14.0.0",
      requested_launch: {
        headless: "new",
        gl: "angle",
        angle_backend: "swiftshader",
        width_px: WIDTH,
        height_px: HEIGHT,
        device_scale_factor: 1,
      },
    },
    graphics: {
      system_devices: [{
        vendor_id: 1,
        device_id: 2,
        vendor_string: "Test Vendor",
        device_string: "Test Device",
        driver_vendor: "Test Driver Vendor",
        driver_version: "3.0",
      }],
      feature_status: [
        { name: "alpha", status: "enabled" },
        { name: "zeta", status: "disabled" },
      ],
      driver_bug_workarounds: ["first_workaround", "second_workaround"],
      effective_context: {
        api: "webgl2",
        drawing_buffer_width: WIDTH,
        drawing_buffer_height: HEIGHT,
        vendor: "WebGL Vendor",
        renderer: "WebGL Renderer",
        version: "WebGL 2.0",
        shading_language_version: "WebGL GLSL ES 3.00",
        unmasked_vendor: "Unmasked Vendor",
        unmasked_renderer: "Unmasked Renderer",
      },
    },
  };
  const fixedProvenanceRaw = provenanceReceiptJson(fixedProvenance);
  assert(JSON.stringify(JSON.parse(fixedProvenanceRaw)) === fixedProvenanceRaw, "provenance canonical JSON round trip");
  const fixedProvenanceHash = sha256(fixedProvenanceRaw);
  assert(
    fixedProvenanceHash === "d922648d9edcc34d18a487adafce959e93ba8b4d821f42d5c1017c7afe50b31f",
    `fixed provenance vector ${fixedProvenanceHash}`,
  );
  const mutateProvenance = (mutate) => {
    const value = JSON.parse(fixedProvenanceRaw);
    mutate(value);
    return value;
  };
  assertThrows(
    () => provenanceReceiptJson(mutateProvenance((value) => {
      value.build.checkout.dirty = true;
    })),
    /dirty does not match/,
    "provenance checkout dirty binding",
  );
  assertThrows(
    () => provenanceReceiptJson(mutateProvenance((value) => {
      value.build.served_files.reverse();
    })),
    /strictly UTF-8-byte sorted/,
    "provenance served-file order",
  );
  assertThrows(
    () => provenanceReceiptJson(mutateProvenance((value) => {
      value.graphics.feature_status[1].name = "alpha";
    })),
    /sorted and unique/,
    "provenance feature uniqueness",
  );
  assertThrows(
    () => provenanceReceiptJson(mutateProvenance((value) => {
      value.graphics.effective_context.renderer = `bad${String.fromCharCode(0)}`;
    })),
    /control character/,
    "provenance contextual control character",
  );
  assertThrows(
    () => provenanceReceiptJson(mutateProvenance((value) => {
      value.extra = true;
    })),
    /field order/,
    "provenance extra top-level field",
  );
  assertThrows(
    () => provenanceReceiptJson(mutateProvenance((value) => {
      value.binding.capture_manifest.byte_length = MAX_PROTOCOL_BYTES + 1;
    })),
    /integer in/,
    "provenance capture-manifest byte budget",
  );
  assertThrows(
    () => provenanceReceiptJson(mutateProvenance((value) => {
      value.build.served_files = [{
        path: "empty.js",
        byte_length: 0,
        sha256: emptyDigest,
      }];
    })),
    /non-empty aggregate/,
    "provenance non-empty served aggregate",
  );
  assertThrows(
    () => provenanceReceiptJson(mutateProvenance((value) => {
      value.build.toolchain.bevy_version = "0.18.1";
    })),
    /frozen 0\.19\.0/,
    "provenance frozen Bevy version",
  );
  assertThrows(
    () => provenanceReceiptJson(mutateProvenance((value) => {
      value.graphics.feature_status = [];
    })),
    /must contain 1/,
    "provenance non-empty feature status",
  );
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

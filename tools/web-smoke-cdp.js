import fs from "node:fs";
import path from "node:path";

const [portText, mode, ...modeArguments] = process.argv.slice(2);
const port = Number.parseInt(portText, 10);
const PAGE_TARGET_TIMEOUT_MS = 10_000;
const PAGE_TARGET_FETCH_TIMEOUT_MS = 1_000;
const CONNECT_TIMEOUT_MS = 10_000;
const COMMAND_TIMEOUT_MS = 10_000;
const DOM_READY_TIMEOUT_MS = 10_000;
const INTERACTION_TIMEOUT_MS = 20_000;
const INTERACTION_COMMAND_TIMEOUT_MS = INTERACTION_TIMEOUT_MS + 5_000;
const OPEN_READY_TIMEOUT_MS = 30_000;
const OPEN_READY_COMMAND_TIMEOUT_MS = OPEN_READY_TIMEOUT_MS + 5_000;
const MAX_INTERACTION_SCRIPT_BYTES = 1024 * 1024;
const MAX_OPEN_FIXTURE_BYTES = 64 * 1024 * 1024;
const MAX_CAPTURE_HTML_BYTES = 2 * 1024 * 1024;
const MAX_CAPTURE_PNG_BYTES = 32 * 1024 * 1024;
const usage = "usage: node tools/web-smoke-cdp.js PORT camera|accessibility SCRIPT\n"
  + "   or: node tools/web-smoke-cdp.js PORT open MISSING_DIRECTORY "
  + "COMPLETE_DIRECTORY MISSING_PAGE_NAME\n"
  + "   or: node tools/web-smoke-cdp.js PORT capture "
  + "compare|preview|context-loss SCREENSHOT DOCUMENT";
if (
  typeof fetch !== "function"
  || typeof WebSocket !== "function"
  || typeof AbortController !== "function"
) {
  console.error("web smoke requires Node.js with fetch, WebSocket, and AbortController APIs");
  process.exit(2);
}
if (
  !Number.isInteger(port)
  || port < 1
  || port > 65535
  || !["camera", "accessibility", "open", "capture"].includes(mode)
  || (["camera", "accessibility"].includes(mode) && modeArguments.length !== 1)
  || (mode === "open" && modeArguments.length !== 3)
  || (mode === "capture" && modeArguments.length !== 3)
) {
  console.error(usage);
  process.exit(2);
}

const checkedRegularFile = (candidate, maximumBytes, expectedSuffix) => {
  if (!path.isAbsolute(candidate)) {
    throw new Error("web smoke file arguments must be absolute paths");
  }
  const canonical = fs.realpathSync(candidate);
  const metadata = fs.statSync(canonical);
  if (!metadata.isFile() || metadata.size < 1 || metadata.size > maximumBytes) {
    throw new Error("web smoke file argument is not a bounded regular file");
  }
  if (expectedSuffix && !canonical.toLowerCase().endsWith(expectedSuffix)) {
    throw new Error(`web smoke file argument must end in ${expectedSuffix}`);
  }
  return canonical;
};

const checkedPortablePageName = (candidate) => {
  if (
    candidate !== path.basename(candidate)
    || !candidate.toLowerCase().endsWith(".png")
    || candidate.length > 255
    || candidate.includes("/")
    || candidate.includes("\\")
    || [...candidate].some((character) => character.charCodeAt(0) < 32)
  ) {
    throw new Error("web smoke missing page name must be one portable PNG basename");
  }
  return candidate;
};

const checkedOpenDirectory = (candidate, expectedExtensions) => {
  if (!path.isAbsolute(candidate)) {
    throw new Error("web smoke Open directory arguments must be absolute paths");
  }
  const canonical = fs.realpathSync(candidate);
  if (!fs.statSync(canonical).isDirectory()) {
    throw new Error("web smoke Open argument is not a directory");
  }
  const entries = fs.readdirSync(canonical, { withFileTypes: true });
  if (entries.length !== expectedExtensions.length || entries.some((entry) => !entry.isFile())) {
    throw new Error("web smoke Open directory has unexpected entries");
  }
  let totalBytes = 0;
  const names = [];
  const extensions = [];
  for (const entry of entries) {
    const file = path.join(canonical, entry.name);
    const metadata = fs.statSync(file);
    if (metadata.size < 1 || metadata.size > MAX_OPEN_FIXTURE_BYTES) {
      throw new Error("web smoke Open directory contains an unbounded file");
    }
    totalBytes += metadata.size;
    names.push(entry.name);
    extensions.push(path.extname(entry.name).toLowerCase());
  }
  if (
    totalBytes > MAX_OPEN_FIXTURE_BYTES
    || extensions.sort().join(",") !== [...expectedExtensions].sort().join(",")
  ) {
    throw new Error("web smoke Open directory has the wrong bounded fixture shape");
  }
  return { canonical, names: names.sort() };
};

const checkedCaptureOutput = (candidate, expectedSuffix) => {
  if (!path.isAbsolute(candidate) || !candidate.toLowerCase().endsWith(expectedSuffix)) {
    throw new Error(`web smoke capture output must be an absolute ${expectedSuffix} path`);
  }
  const parent = fs.realpathSync(path.dirname(candidate));
  const output = path.join(parent, path.basename(candidate));
  if (!fs.statSync(parent).isDirectory() || fs.existsSync(output)) {
    throw new Error("web smoke capture output parent is invalid or output already exists");
  }
  return output;
};

let scriptPath;
let openDirectoryPaths;
let openDirectoryFileNames;
let missingPageName;
let captureKind;
let captureScreenshotPath;
let captureDocumentPath;
try {
  if (mode === "open") {
    const missing = checkedOpenDirectory(modeArguments[0], [".atlas", ".json"]);
    const complete = checkedOpenDirectory(modeArguments[1], [".atlas", ".json", ".png"]);
    missingPageName = checkedPortablePageName(modeArguments[2]);
    const expectedCompleteNames = [...missing.names, missingPageName].sort();
    if (
      missing.canonical === complete.canonical
      || path.dirname(missing.canonical) !== path.dirname(complete.canonical)
      || complete.names.join(",") !== expectedCompleteNames.join(",")
    ) {
      throw new Error("web smoke Open directories are not one missing-page fixture pair");
    }
    openDirectoryPaths = [missing.canonical, complete.canonical];
    openDirectoryFileNames = [missing.names, complete.names];
  } else if (mode === "capture") {
    if (!["compare", "preview", "context-loss"].includes(modeArguments[0])) {
      throw new Error("web smoke capture kind is invalid");
    }
    captureKind = modeArguments[0];
    captureScreenshotPath = checkedCaptureOutput(modeArguments[1], ".png");
    captureDocumentPath = checkedCaptureOutput(modeArguments[2], ".html");
  } else {
    scriptPath = checkedRegularFile(
      path.resolve(modeArguments[0]),
      MAX_INTERACTION_SCRIPT_BYTES,
      ".js",
    );
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(2);
}

const delay = (milliseconds) => new Promise((resolve) => {
  setTimeout(resolve, milliseconds);
});

async function pageTarget() {
  const deadline = Date.now() + PAGE_TARGET_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const controller = new AbortController();
    const fetchTimeout = setTimeout(
      () => controller.abort(),
      Math.max(1, Math.min(PAGE_TARGET_FETCH_TIMEOUT_MS, deadline - Date.now())),
    );
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, {
        signal: controller.signal,
      });
      if (response.ok) {
        const targets = await response.json();
        const target = targets.find((candidate) => (
          candidate.type === "page"
          && candidate.url.startsWith("http://127.0.0.1:")
          && candidate.webSocketDebuggerUrl
        ));
        if (target) return target;
      }
    } catch (_error) {
      // Chrome may still be publishing its debugging endpoint.
    } finally {
      clearTimeout(fetchTimeout);
    }
    await delay(Math.min(50, Math.max(0, deadline - Date.now())));
  }
  throw new Error("Chrome did not publish the Spinal page target");
}

async function connect(url) {
  const socket = new WebSocket(url);
  await new Promise((resolve, reject) => {
    const cleanup = () => {
      clearTimeout(timeout);
      socket.removeEventListener("open", onOpen);
      socket.removeEventListener("error", onError);
    };
    const onOpen = () => {
      cleanup();
      resolve();
    };
    const onError = () => {
      cleanup();
      reject(new Error("Chrome DevTools connection failed"));
    };
    const timeout = setTimeout(() => {
      cleanup();
      reject(new Error("Chrome DevTools connection timed out"));
    }, CONNECT_TIMEOUT_MS);
    socket.addEventListener("open", onOpen, { once: true });
    socket.addEventListener("error", onError, { once: true });
  });
  return socket;
}

async function run() {
  const target = await pageTarget();
  const socket = await connect(target.webSocketDebuggerUrl);
  let nextId = 0;
  const pending = new Map();
  const rejectPending = (error) => {
    for (const { reject, timeout } of pending.values()) {
      clearTimeout(timeout);
      reject(error);
    }
    pending.clear();
  };
  socket.addEventListener("close", () => {
    rejectPending(new Error("Chrome DevTools connection closed"));
  });
  socket.addEventListener("error", () => {
    rejectPending(new Error("Chrome DevTools connection failed"));
  });
  socket.addEventListener("message", (event) => {
    const encoded = typeof event.data === "string"
      ? event.data
      : Buffer.from(event.data).toString("utf8");
    const message = JSON.parse(encoded);
    if (!message.id || !pending.has(message.id)) return;
    const { resolve, reject, timeout, method } = pending.get(message.id);
    pending.delete(message.id);
    clearTimeout(timeout);
    if (message.error) {
      const detail = message.error.message || "command failed without a message";
      reject(new Error(`Chrome DevTools ${method} failed: ${detail}`));
    } else {
      resolve(message.result);
    }
  });

  const command = (
    method,
    params = {},
    timeoutMilliseconds = COMMAND_TIMEOUT_MS,
  ) => new Promise((resolve, reject) => {
    if (socket.readyState !== WebSocket.OPEN) {
      reject(new Error(`Chrome DevTools connection is not open for ${method}`));
      return;
    }
    nextId += 1;
    const id = nextId;
    const timeout = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Chrome DevTools command timed out: ${method}`));
    }, timeoutMilliseconds);
    pending.set(id, { resolve, reject, timeout, method });
    try {
      socket.send(JSON.stringify({ id, method, params }));
    } catch (error) {
      pending.delete(id);
      clearTimeout(timeout);
      reject(error);
    }
  });
  const evaluate = async (expression, timeoutMilliseconds = COMMAND_TIMEOUT_MS) => {
    const response = await command("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
      userGesture: true,
    }, timeoutMilliseconds);
    if (response.exceptionDetails) {
      const detail = response.exceptionDetails.exception?.description
        || response.exceptionDetails.text
        || "browser evaluation failed";
      throw new Error(detail);
    }
    return response.result.value;
  };
  const waitForStableShell = async () => {
    const deadline = Date.now() + DOM_READY_TIMEOUT_MS;
    let consecutiveReadyPolls = 0;
    while (Date.now() < deadline) {
      try {
        const ready = await evaluate(`(() => (
          document.readyState !== "loading"
          && Boolean(document.getElementById("spinal-app"))
          && Boolean(document.getElementById("spinal-status"))
        ))()`);
        consecutiveReadyPolls = ready ? consecutiveReadyPolls + 1 : 0;
        if (consecutiveReadyPolls >= 2) return;
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        if (
          !message.includes("Execution context was destroyed")
          && !message.includes("Inspected target navigated")
        ) throw error;
        consecutiveReadyPolls = 0;
      }
      await delay(Math.min(100, Math.max(0, deadline - Date.now())));
    }
    throw new Error("Spinal browser shell did not become stable");
  };

  try {
    await command("Runtime.enable");
    await waitForStableShell();
    if (mode === "open") {
      await command("DOM.enable");
      const initial = await evaluate(`new Promise((resolve, reject) => {
        const deadline = Date.now() + ${DOM_READY_TIMEOUT_MS};
        const visible = (element) => {
          if (!element || element.hidden) return false;
          const style = getComputedStyle(element);
          const bounds = element.getBoundingClientRect();
          return style.display !== "none"
            && style.visibility !== "hidden"
            && bounds.width > 0
            && bounds.height > 0;
        };
        const poll = () => {
          const app = document.getElementById("spinal-app");
          const status = document.getElementById("spinal-status");
          const panel = document.getElementById("spinal-open-panel");
          const form = document.getElementById("spinal-open-form");
          const input = document.getElementById("spinal-open-files");
          const submit = document.getElementById("spinal-open-submit");
          const error = document.getElementById("spinal-open-error");
          const viewer = document.getElementById("spinal-viewer");
          if (
            app
            && status?.dataset.state === "open"
            && panel
            && form
            && input
            && submit
            && !input.disabled
            && !submit.disabled
            && error
            && viewer
          ) {
            const panelLabelId = panel.getAttribute("aria-labelledby");
            const describedBy = new Set(
              input.getAttribute("aria-describedby")?.split(/\\s+/).filter(Boolean) || [],
            );
            input.focus();
            const inputFocused = document.activeElement === input;
            submit.focus();
            resolve({
              state: status.dataset.state,
              statusText: status.textContent?.trim() || "",
              hasManifest: app.hasAttribute("data-spinal-manifest"),
              panelVisible: visible(panel),
              viewerHidden: !visible(viewer),
              formContained: panel.contains(form),
              inputType: input.getAttribute("type"),
              inputMultiple: input.multiple,
              inputDirectory: input.hasAttribute("webkitdirectory"),
              inputEnabled: !input.disabled,
              inputName: [...input.labels]
                .map((label) => label.textContent?.trim() || "")
                .join(" ")
                .trim(),
              inputDescribed: ["spinal-open-help", "spinal-open-error"]
                .every((id) => describedBy.has(id) && document.getElementById(id)),
              inputFocused,
              inputTargetHeight: input.getBoundingClientRect().height,
              submitName: submit.getAttribute("aria-label")?.trim()
                || submit.textContent?.trim()
                || "",
              submitEnabled: !submit.disabled,
              submitFocused: document.activeElement === submit,
              submitTargetHeight: submit.getBoundingClientRect().height,
              errorRole: error.getAttribute("role"),
              errorTabIndex: error.getAttribute("tabindex"),
              errorHidden: !visible(error),
              panelName: panelLabelId
                ? document.getElementById(panelLabelId)?.textContent?.trim() || ""
                : panel.getAttribute("aria-label")?.trim() || "",
              statusRole: status.getAttribute("role"),
              statusLive: status.getAttribute("aria-live"),
              statusAtomic: status.getAttribute("aria-atomic"),
              ariaBusy: app.getAttribute("aria-busy"),
            });
          } else if (Date.now() >= deadline) {
            reject(new Error("accessible Open shell did not become ready"));
          } else {
            setTimeout(poll, 50);
          }
        };
        poll();
      })`, DOM_READY_TIMEOUT_MS + 5_000);
      if (
        initial.state !== "open"
        || initial.statusText !== "Choose a runtime-export directory to preview."
        || initial.hasManifest
        || !initial.panelVisible
        || !initial.viewerHidden
        || !initial.formContained
        || initial.inputType !== "file"
        || !initial.inputMultiple
        || !initial.inputDirectory
        || !initial.inputEnabled
        || !initial.inputName
        || !initial.inputDescribed
        || !initial.inputFocused
        || initial.inputTargetHeight < 44
        || !/open/i.test(initial.submitName)
        || !initial.submitEnabled
        || !initial.submitFocused
        || initial.submitTargetHeight < 44
        || initial.errorRole !== "alert"
        || initial.errorTabIndex !== "-1"
        || !initial.errorHidden
        || !initial.panelName
        || initial.statusRole !== "status"
        || initial.statusLive !== "polite"
        || initial.statusAtomic !== "true"
        || initial.ariaBusy !== "false"
      ) {
        throw new Error(`Open shell accessibility contract failed: ${JSON.stringify(initial)}`);
      }

      const setOpenDirectory = async (directoryIndex) => {
        const { root } = await command("DOM.getDocument", { depth: 1, pierce: false });
        const { nodeId } = await command("DOM.querySelector", {
          nodeId: root.nodeId,
          selector: "#spinal-open-files",
        });
        if (!nodeId) throw new Error("Open file input disappeared");
        await command("DOM.setFileInputFiles", {
          nodeId,
          files: [openDirectoryPaths[directoryIndex]],
        });
        const expectedNames = JSON.stringify(openDirectoryFileNames[directoryIndex]);
        await evaluate(`new Promise((resolve, reject) => {
          const deadline = Date.now() + ${DOM_READY_TIMEOUT_MS};
          const expectedNames = ${expectedNames};
          const poll = () => {
            const input = document.getElementById("spinal-open-files");
            const files = input?.files ? [...input.files] : [];
            const actualNames = files.map((file) => file.name).sort();
            if (JSON.stringify(actualNames) === JSON.stringify(expectedNames)) {
              const relativeParts = files.map((file) => file.webkitRelativePath.split("/"));
              const roots = new Set(relativeParts.map((parts) => parts[0]));
              const rooted = relativeParts.every((parts, index) => (
                parts.length >= 2 && parts.at(-1) === files[index].name
              ));
              if (!rooted || roots.size !== 1) {
                reject(new Error("CDP did not expose one rooted Open directory"));
              } else {
                resolve(true);
              }
            } else if (files.length > expectedNames.length) {
              reject(new Error("CDP exposed unexpected Open directory entries"));
            } else if (Date.now() >= deadline) {
              reject(new Error("CDP did not populate the Open directory in time"));
            } else {
              setTimeout(poll, 25);
            }
          };
          poll();
        })`, DOM_READY_TIMEOUT_MS + 5_000);
      };
      const submitOpen = () => evaluate(`(() => {
        const form = document.getElementById("spinal-open-form");
        const submit = document.getElementById("spinal-open-submit");
        if (!form || !submit || submit.disabled) {
          throw new Error("Open form is not actionable");
        }
        form.requestSubmit(submit);
        return true;
      })()`);

      await setOpenDirectory(0);
      await submitOpen();
      const rejected = await evaluate(`new Promise((resolve, reject) => {
        const deadline = Date.now() + ${INTERACTION_TIMEOUT_MS};
        const visible = (element) => {
          if (!element || element.hidden) return false;
          const style = getComputedStyle(element);
          const bounds = element.getBoundingClientRect();
          return style.display !== "none"
            && style.visibility !== "hidden"
            && bounds.width > 0
            && bounds.height > 0;
        };
        const poll = () => {
          const app = document.getElementById("spinal-app");
          const status = document.getElementById("spinal-status");
          const panel = document.getElementById("spinal-open-panel");
          const input = document.getElementById("spinal-open-files");
          const submit = document.getElementById("spinal-open-submit");
          const error = document.getElementById("spinal-open-error");
          const viewer = document.getElementById("spinal-viewer");
          if (app?.getAttribute("data-spinal-command-capability")) {
            reject(new Error("invalid Open selection launched the viewer"));
          } else if (
            status?.dataset.state === "open"
            && visible(error)
            && error.textContent?.trim()
          ) {
            resolve({
              state: status.dataset.state,
              statusText: status.textContent?.trim() || "",
              errorText: error.textContent.trim(),
              errorFocused: document.activeElement === error,
              inputCleared: input?.files?.length === 0,
              inputEnabled: input?.disabled === false,
              submitEnabled: submit?.disabled === false,
              panelVisible: visible(panel),
              viewerHidden: !visible(viewer),
              title: document.title,
            });
          } else if (Date.now() >= deadline) {
            reject(new Error("missing-page Open selection was not rejected"));
          } else {
            setTimeout(poll, 50);
          }
        };
        poll();
      })`, INTERACTION_COMMAND_TIMEOUT_MS);
      const failureText = `${rejected.statusText}\n${rejected.errorText}`;
      const fixtureDirectory = path.dirname(openDirectoryPaths[0]);
      const rejectionContract = {
        state: rejected.state,
        statusText: rejected.statusText,
        mentionsMissingPage: failureText.includes(missingPageName),
        errorFocused: rejected.errorFocused,
        inputCleared: rejected.inputCleared,
        inputEnabled: rejected.inputEnabled,
        submitEnabled: rejected.submitEnabled,
        panelVisible: rejected.panelVisible,
        viewerHidden: rejected.viewerHidden,
        title: rejected.title,
      };
      if (
        rejectionContract.state !== "open"
        || rejectionContract.statusText !== "Choose another runtime-export directory."
        || !rejectionContract.mentionsMissingPage
        || !rejectionContract.errorFocused
        || !rejectionContract.inputCleared
        || !rejectionContract.inputEnabled
        || !rejectionContract.submitEnabled
        || !rejectionContract.panelVisible
        || !rejectionContract.viewerHidden
        || rejectionContract.title !== "Spinal — Open"
      ) {
        throw new Error(
          `missing-page Open rejection contract failed: ${JSON.stringify(rejectionContract)}`,
        );
      }
      if (
        openDirectoryPaths.some((directory) => failureText.includes(directory))
        || failureText.includes(fixtureDirectory)
      ) {
        throw new Error("Open rejection exposed a host filesystem path");
      }

      await setOpenDirectory(1);
      await submitOpen();
      const ready = await evaluate(`new Promise((resolve, reject) => {
        const deadline = Date.now() + ${OPEN_READY_TIMEOUT_MS};
        const visible = (element) => {
          if (!element || element.hidden) return false;
          const style = getComputedStyle(element);
          const bounds = element.getBoundingClientRect();
          return style.display !== "none"
            && style.visibility !== "hidden"
            && bounds.width > 0
            && bounds.height > 0;
        };
        const poll = () => {
          const app = document.getElementById("spinal-app");
          const status = document.getElementById("spinal-status");
          if (status?.dataset.state === "blocked") {
            reject(new Error("corrected Open selection entered a blocked state"));
          } else if (
            status?.dataset.state === "open"
            && visible(document.getElementById("spinal-open-error"))
          ) {
            reject(new Error("corrected Open directory was rejected"));
          } else if (status?.dataset.state === "ready") {
            const play = document.getElementById("spinal-play-toggle");
            resolve({
              state: status.dataset.state,
              statusText: status.textContent?.trim() || "",
              mode: app?.dataset.spinalMode,
              hasManifest: app?.hasAttribute("data-spinal-manifest"),
              hasCommandCapability: Boolean(
                app?.getAttribute("data-spinal-command-capability"),
              ),
              panelHidden: !visible(document.getElementById("spinal-open-panel")),
              viewerVisible: visible(document.getElementById("spinal-viewer")),
              playText: play?.textContent?.trim() || "",
              playName: play?.getAttribute("aria-label") || "",
              playEnabled: play?.disabled === false,
              primaryLabel: document.getElementById("spinal-primary-label")
                ?.textContent?.trim() || "",
              canvasName: document.getElementById("spinal-canvas")
                ?.getAttribute("aria-label") || "",
              title: document.title,
              ariaBusy: app?.getAttribute("aria-busy"),
            });
          } else if (Date.now() >= deadline) {
            reject(new Error(
              "corrected Open selection did not reach Ready Preview from "
                + (status?.dataset.state || "missing"),
            ));
          } else {
            setTimeout(poll, 50);
          }
        };
        poll();
      })`, OPEN_READY_COMMAND_TIMEOUT_MS);
      if (
        ready.state !== "ready"
        || !/^Ready\b/.test(ready.statusText)
        || ready.mode !== "preview"
        || ready.hasManifest
        || !ready.hasCommandCapability
        || !ready.panelHidden
        || !ready.viewerVisible
        || ready.playText !== "Play"
        || ready.playName !== "Play"
        || !ready.playEnabled
        || ready.primaryLabel !== "Preview"
        || ready.canvasName !== "Spinal preview viewport."
        || ready.title !== "Spinal — Preview"
        || ready.ariaBusy !== "false"
      ) {
        throw new Error(`corrected Open readiness contract failed: ${JSON.stringify(ready)}`);
      }
      console.log(JSON.stringify({
        mode,
        result: {
          accessibleOpen: true,
          missingPageRejectedBeforeLaunch: true,
          hostPathPrivate: true,
          retryReadyPreview: true,
          initiallyPaused: true,
        },
      }));
    } else if (mode === "capture") {
      await command("Page.enable");
      const expectedKind = JSON.stringify(captureKind);
      const captureState = await evaluate(`new Promise((resolve, reject) => {
        const deadline = Date.now() + ${INTERACTION_TIMEOUT_MS};
        const expectedKind = ${expectedKind};
        const poll = () => {
          const app = document.getElementById("spinal-app");
          const status = document.getElementById("spinal-status");
          const primary = document.getElementById("spinal-primary-label");
          const comparison = document.getElementById("spinal-comparison-label");
          const ready = status?.dataset.state === "ready";
          const reached = expectedKind === "compare"
            ? ready
              && app?.dataset.spinalMode === "compare"
              && primary?.textContent?.trim() === "Primary"
              && comparison?.textContent?.trim() === "Comparison — setup pose"
              && document.body.textContent?.includes(
                "Comparison does not contain animation “sway”; showing setup pose in that pane.",
              )
            : expectedKind === "preview"
              ? ready
                && app?.dataset.spinalMode === "preview"
                && primary?.textContent?.trim() === "Preview"
                && comparison?.hidden === true
              : status?.dataset.state === "blocked"
                && app?.dataset.spinalGraphicsBlocked === "true"
                && status.textContent?.includes("browser graphics were lost");
          if (reached) {
            resolve({
              kind: expectedKind,
              state: status.dataset.state,
              mode: app?.dataset.spinalMode || "missing",
            });
          } else if (Date.now() >= deadline) {
            reject(new Error("capture page did not reach its semantic state"));
          } else {
            setTimeout(poll, 50);
          }
        };
        poll();
      })`, INTERACTION_COMMAND_TIMEOUT_MS);
      await evaluate(`new Promise((resolve) => {
        let complete = false;
        const done = () => {
          if (complete) return;
          complete = true;
          resolve(true);
        };
        requestAnimationFrame(() => requestAnimationFrame(done));
        setTimeout(done, 500);
      })`);
      const documentHtml = await evaluate(
        '`<!doctype html>\n${document.documentElement.outerHTML}`',
      );
      if (
        typeof documentHtml !== "string"
        || Buffer.byteLength(documentHtml, "utf8") < 1
        || Buffer.byteLength(documentHtml, "utf8") > MAX_CAPTURE_HTML_BYTES
      ) {
        throw new Error("captured browser document is not bounded HTML");
      }
      const screenshot = await command("Page.captureScreenshot", {
        format: "png",
        fromSurface: true,
        captureBeyondViewport: false,
        optimizeForSpeed: true,
      });
      if (typeof screenshot.data !== "string" || screenshot.data.length > MAX_CAPTURE_PNG_BYTES * 2) {
        throw new Error("captured browser screenshot is not bounded base64");
      }
      const png = Buffer.from(screenshot.data, "base64");
      const pngSignature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
      if (
        png.length < pngSignature.length
        || png.length > MAX_CAPTURE_PNG_BYTES
        || !png.subarray(0, pngSignature.length).equals(pngSignature)
      ) {
        throw new Error("captured browser screenshot is not a bounded PNG");
      }
      fs.writeFileSync(captureDocumentPath, documentHtml, { encoding: "utf8", flag: "wx", mode: 0o600 });
      fs.writeFileSync(captureScreenshotPath, png, { flag: "wx", mode: 0o600 });
      console.log(JSON.stringify({ mode, result: captureState }));
    } else {
      await evaluate(fs.readFileSync(scriptPath, "utf8"));
      const expression = mode === "camera"
        ? `new Promise((resolve, reject) => {
          const deadline = Date.now() + ${INTERACTION_TIMEOUT_MS};
          const poll = () => {
            const app = document.getElementById("spinal-app");
            if (app?.dataset.spinalSmokePhase === "done") {
              resolve({
                phase: app.dataset.spinalSmokePhase,
                keyboardHandled: app.dataset.spinalSmokeKeyboardHandled,
                mutated: app.dataset.spinalSmokeMutated,
                refit: app.dataset.spinalSmokeRefit,
                synchronized: app.dataset.spinalCameraSynchronized,
                zoom: app.dataset.spinalCameraZoom,
                panned: app.dataset.spinalCameraPanned,
                baseFitSynchronized: app.dataset.spinalBaseFitSynchronized,
                refitRequested: app.dataset.spinalSmokeRefitRequested,
                refitDisabled: app.dataset.spinalSmokeRefitDisabled,
              });
            } else if (Date.now() >= deadline) {
              reject(new Error("camera interaction did not finish"));
            } else {
              setTimeout(poll, 50);
            }
          };
          poll();
        })`
      : `new Promise((resolve, reject) => {
          const deadline = Date.now() + ${INTERACTION_TIMEOUT_MS};
          const poll = () => {
            const app = document.getElementById("spinal-app");
            if (app?.dataset.spinalA11ySmoke) {
              resolve({
                result: app.dataset.spinalA11ySmoke,
                width: app.dataset.spinalA11ySmokeWidth,
                checks: app.dataset.spinalA11ySmokeChecks,
                failures: app.dataset.spinalA11ySmokeFailures,
              });
            } else if (Date.now() >= deadline) {
              reject(new Error("accessibility preflight did not finish"));
            } else {
              setTimeout(poll, 50);
            }
          };
          poll();
          })`;
      const result = await evaluate(expression, INTERACTION_COMMAND_TIMEOUT_MS);
      if (mode === "camera") {
        const expected = {
          phase: "done",
          keyboardHandled: "true",
          mutated: "125,true,true",
          refit: "100,false,true",
          synchronized: "true",
          zoom: "100",
          panned: "false",
          baseFitSynchronized: "true",
          refitRequested: "true",
          refitDisabled: "false",
        };
        for (const [key, value] of Object.entries(expected)) {
          if (result[key] !== value) {
            throw new Error(`camera ${key} was ${JSON.stringify(result[key])}; expected ${value}`);
          }
        }
      } else if (
        result.result !== "passed"
        || result.width !== "500"
        || result.checks !== (
          "semantics,focus,narrow-layout,horizontal-overflow,quiet-status,contrast"
        )
        || result.failures !== ""
      ) {
        throw new Error(`accessibility preflight failed: ${JSON.stringify(result)}`);
      }
      console.log(JSON.stringify({ mode, result }));
    }
  } finally {
    rejectPending(new Error("Chrome DevTools connection closed by smoke driver"));
    socket.close();
  }
}

run().catch((error) => {
  console.error(error instanceof Error ? error.stack : String(error));
  process.exit(1);
});

import fs from "node:fs";

const [portText, mode, scriptPath] = process.argv.slice(2);
const port = Number.parseInt(portText, 10);
const PAGE_TARGET_TIMEOUT_MS = 10_000;
const PAGE_TARGET_FETCH_TIMEOUT_MS = 1_000;
const CONNECT_TIMEOUT_MS = 10_000;
const COMMAND_TIMEOUT_MS = 10_000;
const DOM_READY_TIMEOUT_MS = 10_000;
const INTERACTION_TIMEOUT_MS = 20_000;
const INTERACTION_COMMAND_TIMEOUT_MS = INTERACTION_TIMEOUT_MS + 5_000;
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
  || !["camera", "accessibility"].includes(mode)
  || !scriptPath
) {
  console.error("usage: node tools/web-smoke-cdp.js PORT camera|accessibility SCRIPT");
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
    const { resolve, reject, timeout } = pending.get(message.id);
    pending.delete(message.id);
    clearTimeout(timeout);
    if (message.error) {
      reject(new Error(message.error.message || "Chrome DevTools command failed"));
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
    pending.set(id, { resolve, reject, timeout });
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

  try {
    await command("Runtime.enable");
    await evaluate(`new Promise((resolve, reject) => {
      const deadline = Date.now() + ${DOM_READY_TIMEOUT_MS};
      const poll = () => {
        if (
          document.readyState !== "loading"
          && document.getElementById("spinal-app")
          && document.getElementById("spinal-status")
        ) {
          resolve(true);
        } else if (Date.now() >= deadline) {
          reject(new Error("Spinal browser shell did not become ready"));
        } else {
          setTimeout(poll, 50);
        }
      };
      poll();
    })`, DOM_READY_TIMEOUT_MS + 5_000);
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
    } else {
      if (
        result.result !== "passed"
        || result.width !== "500"
        || result.checks !== (
          "semantics,focus,narrow-layout,horizontal-overflow,quiet-status,contrast"
        )
        || result.failures !== ""
      ) {
        throw new Error(`accessibility preflight failed: ${JSON.stringify(result)}`);
      }
    }
    console.log(JSON.stringify({ mode, result }));
  } finally {
    rejectPending(new Error("Chrome DevTools connection closed by smoke driver"));
    socket.close();
  }
}

run().catch((error) => {
  console.error(error instanceof Error ? error.stack : String(error));
  process.exit(1);
});

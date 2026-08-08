(() => {
  "use strict";

  const app = document.getElementById("spinal-app");
  const canvas = document.getElementById("spinal-canvas");
  const status = document.getElementById("spinal-status");
  const zoomIn = document.getElementById("spinal-zoom-in");
  const fit = document.getElementById("spinal-refit");
  if (!app || !canvas || !status || !zoomIn || !fit) return;

  const state = () => [
    app.dataset.spinalCameraZoom,
    app.dataset.spinalCameraPanned,
    app.dataset.spinalCameraSynchronized,
  ].join(",");
  let phase = "ready";

  const setPhase = (next) => {
    phase = next;
    app.dataset.spinalSmokePhase = next;
  };
  const advance = () => {
    if (
      phase === "ready"
      && status.dataset.state === "ready"
      && !zoomIn.disabled
      && !fit.disabled
      && state() === "100,false,true"
    ) {
      setPhase("mutated");
      zoomIn.click();
      canvas.focus();
      const event = new KeyboardEvent("keydown", {
        key: "ArrowRight",
        bubbles: true,
        cancelable: true,
      });
      app.dataset.spinalSmokeKeyboardHandled =
        String(!canvas.dispatchEvent(event));
    } else if (phase === "mutated" && state() === "125,true,true") {
      app.dataset.spinalSmokeMutated = state();
      setPhase("refit");
      fit.click();
    } else if (phase === "refit" && state() === "100,false,true") {
      app.dataset.spinalSmokeRefit = state();
      setPhase("done");
      observer.disconnect();
    }
  };

  const observer = new MutationObserver(advance);
  observer.observe(status, {
    attributes: true,
    attributeFilter: ["data-state"],
  });
  observer.observe(app, {
    attributes: true,
    attributeFilter: [
      "data-spinal-camera-zoom",
      "data-spinal-camera-panned",
      "data-spinal-camera-synchronized",
    ],
  });
  setPhase("ready");
  advance();
})();

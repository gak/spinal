(() => {
  "use strict";

  const app = document.getElementById("spinal-app");
  const status = document.getElementById("spinal-status");
  if (!app || !status) return;

  let ready = false;
  let readyMutations = 0;
  let scheduled = false;
  const observer = new MutationObserver(() => {
    if (ready) readyMutations += 1;
    schedule();
  });

  const fail = (failures, condition, message) => {
    if (!condition) failures.push(message);
  };
  const visible = (element) => {
    const style = getComputedStyle(element);
    const box = element.getBoundingClientRect();
    return !element.hidden
      && style.display !== "none"
      && style.visibility !== "hidden"
      && box.width > 0
      && box.height > 0;
  };
  const parseRgb = (value) => {
    const match = value.match(
      /rgba?\(\s*(\d+(?:\.\d+)?)[,\s]+(\d+(?:\.\d+)?)[,\s]+(\d+(?:\.\d+)?)/,
    );
    return match ? match.slice(1, 4).map(Number) : null;
  };
  const luminance = (rgb) => {
    const channels = rgb.map((channel) => {
      const value = channel / 255;
      return value <= 0.04045
        ? value / 12.92
        : ((value + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
  };
  const contrast = (foreground, background) => {
    const light = Math.max(luminance(foreground), luminance(background));
    const dark = Math.min(luminance(foreground), luminance(background));
    return (light + 0.05) / (dark + 0.05);
  };
  const hasName = (element) => {
    const labelled = element.getAttribute("aria-labelledby")
      ?.split(/\s+/)
      .filter(Boolean)
      .map((id) => document.getElementById(id)?.textContent?.trim() || "")
      .join(" ")
      .trim();
    const label = element.getAttribute("aria-label")?.trim();
    const explicit = element.id
      ? document.querySelector(`label[for="${CSS.escape(element.id)}"]`)?.textContent?.trim()
      : "";
    return Boolean(labelled || label || explicit || element.textContent?.trim());
  };

  const audit = () => {
    scheduled = false;
    if (status.dataset.state !== "ready") return;
    ready = true;
    const failures = [];
    const root = document.documentElement;
    const width = root.clientWidth;

    const ids = [...document.querySelectorAll("[id]")].map((element) => element.id);
    fail(failures, ids.length === new Set(ids).size, "duplicate-id");

    for (const element of document.querySelectorAll("[for], [aria-controls], [aria-describedby], [aria-labelledby]")) {
      for (const attribute of ["for", "aria-controls", "aria-describedby", "aria-labelledby"]) {
        const value = element.getAttribute(attribute);
        if (!value) continue;
        for (const id of value.split(/\s+/).filter(Boolean)) {
          fail(failures, Boolean(document.getElementById(id)), `missing-reference:${attribute}:${id}`);
        }
      }
    }

    fail(
      failures,
      !document.querySelector('[tabindex]:not([tabindex="0"]):not([tabindex="-1"])'),
      "positive-tabindex",
    );
    const live = [...document.querySelectorAll("[aria-live]")];
    const statusRoles = [...document.querySelectorAll('[role="status"]')];
    fail(failures, live.length === 1 && live[0] === status, "live-region-count");
    fail(
      failures,
      statusRoles.length === 1 && statusRoles[0] === status,
      "status-role-count",
    );
    fail(failures, document.querySelectorAll("output").length === 0, "output-element-count");
    const openAlert = document.getElementById("spinal-open-error");
    fail(
      failures,
      openAlert?.getAttribute("role") === "alert"
        && openAlert.hidden
        && !visible(openAlert),
      "open-alert-hidden-in-viewer",
    );
    fail(
      failures,
      !document.querySelector(
        "#spinal-camera-state[aria-live], #spinal-diagnostics[aria-live], #spinal-diagnostics [aria-live]",
      ),
      "dynamic-detail-live-region",
    );

    const primaryPane = document.getElementById("spinal-primary-pane");
    const comparisonPane = document.getElementById("spinal-comparison-pane");
    const primaryHeading = document.getElementById("spinal-primary-label");
    const comparisonHeading = document.getElementById("spinal-comparison-label");
    const primaryState = document.getElementById("spinal-primary-state");
    const comparisonState = document.getElementById("spinal-comparison-state");
    const primaryTime = document.getElementById("spinal-primary-time");
    const comparisonTime = document.getElementById("spinal-comparison-time");
    const validState = (element, text, state) => (
      element?.tagName === "P"
      && element.textContent?.trim() === text
      && element.dataset.state === state
      && !element.hasAttribute("role")
      && !element.hasAttribute("aria-live")
    );
    const validTime = (element, text, hidden) => (
      element?.tagName === "SPAN"
      && element.textContent?.trim() === text
      && element.hidden === hidden
      && visible(element) === !hidden
      && element.getAttribute("aria-hidden") === "true"
      && !element.hasAttribute("role")
      && !element.hasAttribute("aria-live")
    );
    fail(failures, app.dataset.spinalMode === "compare", "pane-mode");
    fail(
      failures,
      primaryPane?.tagName === "SECTION" && visible(primaryPane),
      "primary-pane-visible",
    );
    fail(
      failures,
      comparisonPane?.tagName === "SECTION" && visible(comparisonPane),
      "comparison-pane-visible",
    );
    fail(
      failures,
      primaryHeading?.tagName === "H2" && primaryHeading.textContent?.trim() === "Primary",
      "primary-pane-heading",
    );
    fail(
      failures,
      comparisonHeading?.tagName === "H2"
        && comparisonHeading.textContent?.trim() === "Comparison",
      "comparison-pane-heading",
    );
    fail(
      failures,
      validState(
        primaryState,
        "Ready — animation “sway” • skin Default",
        "ready",
      ),
      "primary-pane-state",
    );
    fail(
      failures,
      validState(
        comparisonState,
        "Warning — animation “sway” unavailable; setup pose • skin Default",
        "warning",
      ),
      "comparison-pane-state",
    );
    fail(
      failures,
      validTime(primaryTime, "0.000 / 1.000 s", false),
      "primary-pane-time",
    );
    fail(failures, validTime(comparisonTime, "", true), "comparison-pane-time");

    const canvas = document.getElementById("spinal-canvas");
    const expectedCanvasName = app.dataset.spinalMode === "compare"
      ? "Spinal comparison viewport. Primary is left; Comparison is right."
      : "Spinal preview viewport.";
    fail(failures, canvas?.getAttribute("role") === "img", "canvas-role");
    fail(failures, canvas?.getAttribute("tabindex") === "0", "canvas-tabindex");
    fail(failures, canvas?.getAttribute("aria-label") === expectedCanvasName, "canvas-name");
    const descriptions = new Set(
      canvas?.getAttribute("aria-describedby")?.split(/\s+/).filter(Boolean) || [],
    );
    for (const id of [
      "spinal-status",
      "spinal-camera-help",
      "spinal-camera-state",
      "spinal-diagnostics-summary",
    ]) fail(failures, descriptions.has(id), `canvas-description:${id}`);

    const timeline = document.getElementById("spinal-timeline");
    const timelineDisplay = document.getElementById("spinal-timeline-value");
    fail(
      failures,
      timelineDisplay?.tagName === "SPAN"
        && timelineDisplay.getAttribute("aria-hidden") === "true"
        && !timelineDisplay.hidden
        && timelineDisplay.textContent?.trim() === primaryTime?.textContent?.trim(),
      "timeline-display-hidden",
    );
    fail(
      failures,
      /^\d+\.\d{3} of \d+\.\d{3} seconds$/.test(timeline?.getAttribute("aria-valuetext") || ""),
      "timeline-valuetext",
    );
    const cameraState = document.getElementById("spinal-camera-state");
    fail(
      failures,
      cameraState?.tagName === "SPAN"
        && !cameraState.hasAttribute("aria-hidden")
        && !cameraState.hidden
        && visible(cameraState)
        && Boolean(cameraState.textContent?.trim())
        && descriptions.has("spinal-camera-state"),
      "camera-state-accessible",
    );
    const play = document.getElementById("spinal-play-toggle");
    fail(
      failures,
      play?.textContent?.trim() === "Play" && play.getAttribute("aria-label") === "Play",
      "initially-paused",
    );

    fail(failures, width >= 500 && width <= 520, `narrow-width:${width}`);
    fail(failures, root.scrollWidth <= width + 1, `root-overflow:${root.scrollWidth}/${width}`);
    fail(
      failures,
      document.body.scrollWidth <= document.body.clientWidth + 1,
      `body-overflow:${document.body.scrollWidth}/${document.body.clientWidth}`,
    );

    const controls = [...document.querySelectorAll(
      'button:not([disabled]), select:not([disabled]), input:not([disabled]), summary, canvas[tabindex="0"]',
    )].filter(visible);
    fail(failures, controls.length >= 12, `enabled-controls:${controls.length}`);
    for (const control of controls) {
      fail(failures, hasName(control), `unnamed-control:${control.id || control.tagName}`);
      control.focus();
      const box = control.getBoundingClientRect();
      const style = getComputedStyle(control);
      fail(failures, document.activeElement === control, `focus-failed:${control.id || control.tagName}`);
      fail(
        failures,
        box.left >= -1 && box.right <= width + 1,
        `focus-horizontal:${control.id || control.tagName}`,
      );
      const outlineWidth = Number.parseFloat(style.outlineWidth) || 0;
      fail(
        failures,
        outlineWidth >= 2 || style.boxShadow !== "none",
        `focus-indicator:${control.id || control.tagName}`,
      );
    }

    for (const control of document.querySelectorAll("button:not([disabled]), select:not([disabled])")) {
      if (!visible(control)) continue;
      const box = control.getBoundingClientRect();
      fail(failures, box.height >= 44, `target-height:${control.id}:${box.height}`);
    }
    const loopLabel = document.querySelector('label[for="spinal-looping"]');
    fail(failures, (loopLabel?.getBoundingClientRect().height || 0) >= 44, "loop-target-height");

    if (canvas) {
      canvas.focus();
      const outline = parseRgb(getComputedStyle(canvas).outlineColor);
      const frame = document.querySelector(".canvas-frame");
      const background = frame ? parseRgb(getComputedStyle(frame).backgroundColor) : null;
      fail(
        failures,
        Boolean(outline && background && contrast(outline, background) >= 3),
        "canvas-focus-contrast",
      );
    }
    const sampleButton = document.getElementById("spinal-refit");
    if (sampleButton) {
      const style = getComputedStyle(sampleButton);
      const foreground = parseRgb(style.color);
      const background = parseRgb(style.backgroundColor);
      fail(
        failures,
        Boolean(foreground && background && contrast(foreground, background) >= 4.5),
        "button-text-contrast",
      );
    }
    fail(failures, /^Ready\b/.test(status.textContent?.trim() || ""), "noncolor-ready-status");
    fail(
      failures,
      /^Diagnostics\b/.test(document.getElementById("spinal-diagnostics-summary")?.textContent?.trim() || ""),
      "noncolor-diagnostics-status",
    );

    window.setTimeout(() => {
      fail(failures, readyMutations === 0, `ready-live-mutations:${readyMutations}`);
      app.dataset.spinalA11ySmokeWidth = String(width);
      app.dataset.spinalA11ySmokeChecks =
        "semantics,pane-presentation,focus,narrow-layout,horizontal-overflow,quiet-status,contrast";
      app.dataset.spinalA11ySmokeFailures = failures.join("|");
      app.dataset.spinalA11ySmoke = failures.length === 0 ? "passed" : "failed";
      observer.disconnect();
    }, 1000);
  };

  function schedule() {
    if (scheduled || status.dataset.state !== "ready") return;
    scheduled = true;
    window.setTimeout(audit, 500);
  }

  observer.observe(status, { attributes: true, childList: true, subtree: true });
  schedule();
})();

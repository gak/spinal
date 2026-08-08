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
    const live = [...document.querySelectorAll('[aria-live]:not([aria-live="off"])')];
    fail(failures, live.length === 1 && live[0] === status, "live-region-count");
    fail(failures, document.querySelectorAll('[role="status"]').length === 1, "status-role-count");
    fail(
      failures,
      !document.querySelector(
        "#spinal-camera-state[aria-live], #spinal-diagnostics[aria-live], #spinal-diagnostics [aria-live]",
      ),
      "dynamic-detail-live-region",
    );

    const canvas = document.getElementById("spinal-canvas");
    const expectedCanvasName = app.dataset.spinalMode === "compare"
      ? "Spinal comparison viewport. Current is left; Proposed is right."
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
    fail(
      failures,
      /^\d+\.\d{3} of \d+\.\d{3} seconds$/.test(timeline?.getAttribute("aria-valuetext") || ""),
      "timeline-valuetext",
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
        "semantics,focus,narrow-layout,horizontal-overflow,quiet-status,contrast";
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

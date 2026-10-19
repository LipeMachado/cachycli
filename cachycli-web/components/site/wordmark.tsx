"use client";

import { useEffect, useRef, useState } from "react";
import { animate } from "motion/react";

/**
 * Footer wordmark rendered as SVG so it stays crisp at any pixel density (the
 * HTML version rasterised the stroke + mask and looked soft on 1x screens).
 *
 * Default: flat dark letters, no edge. On hover: only the letters under the
 * pointer turn into polished metal and gain a bright edge. No halo.
 */
export default function Wordmark({ text = "CACHYCLI" }: { text?: string }) {
  const svg = useRef<SVGSVGElement>(null);
  const label = useRef<SVGTextElement>(null);
  const spot = useRef<SVGCircleElement>(null);
  const [box, setBox] = useState({ w: 720, h: 150 });

  useEffect(() => {
    const measure = () => {
      const node = label.current;
      if (!node) return;
      const b = node.getBBox();
      setBox({ w: Math.ceil(b.width + 12), h: Math.ceil(b.height + 12) });
    };
    measure();
    // Fonts can arrive after first paint; re-measure then.
    if (typeof document !== "undefined" && "fonts" in document) {
      document.fonts.ready.then(measure).catch(() => {});
    }
  }, []);

  const toUserUnits = (clientX: number, clientY: number) => {
    const node = svg.current;
    if (!node) return { x: -9999, y: -9999 };
    const rect = node.getBoundingClientRect();
    return {
      x: ((clientX - rect.left) / rect.width) * box.w,
      y: ((clientY - rect.top) / rect.height) * box.h,
    };
  };

  const onMove = (event: React.MouseEvent) => {
    const circle = spot.current;
    if (!circle) return;
    const { x, y } = toUserUnits(event.clientX, event.clientY);
    circle.setAttribute("cx", String(x));
    circle.setAttribute("cy", String(y));
    // Fade the highlight back in only when it was faded out.
    if (Number(circle.style.opacity || "1") < 1) {
      animate(circle, { opacity: 1 }, { duration: 0.4, ease: [0.22, 1, 0.36, 1] });
    }
  };

  const onLeave = () => {
    const circle = spot.current;
    if (!circle) return;
    // Fade the glow out instead of yanking it away.
    animate(circle, { opacity: 0 }, { duration: 0.65, ease: [0.22, 1, 0.36, 1] });
  };

  return (
    <div className="hidden select-none lg:block" style={{ cursor: "default" }} aria-hidden="true">
      <svg
        ref={svg}
        viewBox={`0 0 ${box.w} ${box.h}`}
        width="100%"
        role="presentation"
        onMouseMove={onMove}
        onMouseLeave={onLeave}
        style={{ display: "block", overflow: "visible", cursor: "default" }}
      >
        <defs>
          <radialGradient id="wm-spot" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="#fff" stopOpacity="1" />
            <stop offset="45%" stopColor="#fff" stopOpacity="0.55" />
            <stop offset="100%" stopColor="#fff" stopOpacity="0" />
          </radialGradient>

          <linearGradient id="wm-metal" x1="0%" y1="0%" x2="100%" y2="0%">
            <stop offset="0%" stopColor="#ffffff" />
            <stop offset="16%" stopColor="#c2c7d2" />
            <stop offset="32%" stopColor="#fdfeff" />
            <stop offset="48%" stopColor="#9aa0ad" />
            <stop offset="64%" stopColor="#f2f5fb" />
            <stop offset="80%" stopColor="#b0b6c2" />
            <stop offset="100%" stopColor="#ffffff" />
            <animateTransform
              attributeName="gradientTransform"
              type="translate"
              values="-0.5 0; 0.5 0; -0.5 0"
              dur="3.2s"
              repeatCount="indefinite"
            />
          </linearGradient>

          <mask id="wm-mask" maskUnits="userSpaceOnUse" x="0" y="0" width={box.w} height={box.h}>
            <rect x="0" y="0" width={box.w} height={box.h} fill="black" />
            <circle ref={spot} cx="-9999" cy="-9999" r="170" fill="url(#wm-spot)" style={{ opacity: 0 }} />
          </mask>
        </defs>

        {/* flat letters, no edge */}
        <text
          ref={label}
          x="6"
          y="72%"
          className="display"
          style={{ fontSize: "100px", letterSpacing: "-0.035em" }}
          fill="#2b2c33"
        >
          {text}
        </text>

        {/* hover state: metallic fill with a bright edge, clipped to the spotlight */}
        <g mask="url(#wm-mask)">
          <text
            x="6"
            y="72%"
            className="display"
            style={{ fontSize: "100px", letterSpacing: "-0.035em" }}
            fill="url(#wm-metal)"
            stroke="#f8faff"
            strokeWidth="1.1"
            paintOrder="stroke"
          >
            {text}
          </text>
        </g>
      </svg>
    </div>
  );
}

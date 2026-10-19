import { MARQUEE_ITEMS } from "@/lib/content";

/** A slow mono marquee strip; pauses on hover, stops for reduced motion. */
export default function Marquee({ items = MARQUEE_ITEMS }: { items?: string[] }) {
  const row = [...items, ...items];
  return (
    <div className="marquee rule-t rule-b overflow-hidden py-4">
      <div className="marquee-track">
        {row.map((item, i) => (
          <span key={`${item}-${i}`} className="mono flex items-center gap-6 pr-6" style={{ color: "var(--ink-60)" }}>
            <span>{item}</span>
            <span aria-hidden="true" style={{ color: "var(--accent)" }}>
              ◆
            </span>
          </span>
        ))}
      </div>
    </div>
  );
}

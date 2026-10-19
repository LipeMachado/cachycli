import type { MetadataRoute } from "next";
import { GUIDE_TOPICS, topicHref } from "@/lib/guide";

const BASE = process.env.NEXT_PUBLIC_SITE_URL ?? "https://cachycli.vercel.app";

export default function sitemap(): MetadataRoute.Sitemap {
  return [
    { url: `${BASE}/`, priority: 1 },
    ...GUIDE_TOPICS.map((topic, i) => ({
      url: `${BASE}${topicHref(topic)}`,
      priority: i < 4 ? 0.9 : 0.6,
    })),
  ];
}

"use client";

import Image from "next/image";
import { useState } from "react";
import { motion } from "motion/react";

const EASE = [0.22, 1, 0.36, 1] as const;

/**
 * Screenshot with a skeleton placeholder and a soft fade-in, so slow images
 * never look broken.
 *
 * `mode="fill"` requires a sized parent (as in the terminal frames, which use
 * `aspect-ratio`); `mode="intrinsic"` keeps the image in flow and takes its
 * height from the picture itself.
 */
export default function Shot({
  src,
  alt,
  width,
  height,
  sizes,
  priority = false,
  className = "",
  quality = 90,
  mode = "fill",
}: {
  src: string;
  alt: string;
  width: number;
  height: number;
  sizes: string;
  priority?: boolean;
  className?: string;
  quality?: number;
  mode?: "fill" | "intrinsic";
}) {
  const [loaded, setLoaded] = useState(false);

  const skeleton = !loaded ? (
    <span
      aria-hidden="true"
      className="absolute inset-0 animate-pulse"
      style={{
        background:
          "linear-gradient(100deg, rgba(242,241,238,0.05) 0%, rgba(242,241,238,0.10) 45%, rgba(242,241,238,0.05) 100%)",
        backgroundSize: "200% 100%",
        animationDuration: "1.6s",
      }}
    />
  ) : null;

  const image = (
    <Image
      src={src}
      alt={alt}
      width={width}
      height={height}
      sizes={sizes}
      quality={quality}
      priority={priority}
      loading={priority ? "eager" : "lazy"}
      onLoad={() => setLoaded(true)}
      className={className}
    />
  );

  if (mode === "fill") {
    return (
      <>
        {skeleton}
        <motion.span
          className="absolute inset-0 block"
          initial={false}
          animate={{ opacity: loaded ? 1 : 0 }}
          transition={{ duration: 0.5, ease: EASE }}
        >
          {image}
        </motion.span>
      </>
    );
  }

  return (
    <>
      {skeleton}
      <motion.span
        className="block"
        initial={false}
        animate={{ opacity: loaded ? 1 : 0 }}
        transition={{ duration: 0.5, ease: EASE }}
      >
        {image}
      </motion.span>
    </>
  );
}

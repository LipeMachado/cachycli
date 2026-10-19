import Link from "next/link";
import type { AnchorHTMLAttributes, ButtonHTMLAttributes, ReactNode } from "react";

export type ButtonVariant = "primary" | "ghost" | "amber" | "link";
export type ButtonSize = "sm" | "md" | "lg";

const VARIANT: Record<ButtonVariant, string> = {
  primary: "btn-primary",
  ghost: "btn-ghost",
  amber: "btn-amber",
  link: "btn-link",
};

const SIZE: Record<ButtonSize, string> = {
  sm: "btn-sm",
  md: "btn-md",
  lg: "btn-lg",
};

function classes(variant: ButtonVariant, size: ButtonSize, className: string) {
  return ["btn", VARIANT[variant], variant === "link" ? "" : SIZE[size], className]
    .filter(Boolean)
    .join(" ");
}

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  trailing?: ReactNode;
};

/** Square-cornered mono button. One component, four variants, three sizes. */
export function Button({
  variant = "primary",
  size = "md",
  className = "",
  trailing,
  children,
  ...rest
}: ButtonProps) {
  return (
    <button type="button" className={classes(variant, size, className)} {...rest}>
      {children}
      {trailing}
    </button>
  );
}

type ButtonLinkProps = AnchorHTMLAttributes<HTMLAnchorElement> & {
  href: string;
  variant?: ButtonVariant;
  size?: ButtonSize;
  external?: boolean;
  trailing?: ReactNode;
};

/** Same button, rendered as a link (internal or external). */
export function ButtonLink({
  href,
  variant = "primary",
  size = "md",
  external = false,
  className = "",
  trailing,
  children,
  ...rest
}: ButtonLinkProps) {
  const cls = classes(variant, size, className);
  if (external) {
    return (
      <a href={href} target="_blank" rel="noreferrer" className={cls} {...rest}>
        {children}
        {trailing}
      </a>
    );
  }
  return (
    <Link href={href} className={cls} {...rest}>
      {children}
      {trailing}
    </Link>
  );
}

import Lenis from "lenis";

let instance: Lenis | null = null;
let locked = false;

/** Registered by the SmoothScroll provider. */
export function setLenis(next: Lenis | null) {
  instance = next;
  if (next && locked) next.stop();
}

/** Lock page scrolling (mobile menu, modals) including Lenis. */
export function lockScroll(next: boolean) {
  locked = next;
  if (next) {
    instance?.stop();
    document.documentElement.classList.add("lenis-stopped");
    document.body.style.overflow = "hidden";
  } else {
    instance?.start();
    document.documentElement.classList.remove("lenis-stopped");
    document.body.style.overflow = "";
  }
}

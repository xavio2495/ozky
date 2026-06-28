// Centralized smooth-scroll + scroll-trigger setup (Lenis + GSAP).
// Initialized once from the root layout; respects prefers-reduced-motion.
import Lenis from 'lenis';
import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';

let lenis: Lenis | null = null;

// Hold ALL GSAP playback until the initial loader lifts. Pages create their intro
// tweens on mount (behind the loader); pausing the global timeline here — at first
// import, before any component mounts — keeps those tweens parked at frame 0 so they
// actually play when the loader calls releaseAnimations(), instead of finishing unseen.
let animationsReleased = false;
if (typeof window !== 'undefined') {
	gsap.globalTimeline.pause();
}

export function releaseAnimations(): void {
	if (animationsReleased) return;
	animationsReleased = true;
	if (typeof window !== 'undefined') gsap.globalTimeline.resume();
}

export function reducedMotion(): boolean {
	return (
		typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches
	);
}

/** Boot Lenis + GSAP ScrollTrigger. Returns a teardown fn. No-op under reduced motion. */
export function initScroll(): () => void {
	if (typeof window === 'undefined') return () => {};

	gsap.registerPlugin(ScrollTrigger);

	if (reducedMotion()) {
		return () => {};
	}

	lenis = new Lenis({
		duration: 1.1,
		easing: (t) => Math.min(1, 1.001 - Math.pow(2, -10 * t)),
		smoothWheel: true
	});

	// Drive Lenis from GSAP's ticker and keep ScrollTrigger in sync.
	lenis.on('scroll', ScrollTrigger.update);
	const onTick = (time: number) => lenis?.raf(time * 1000);
	gsap.ticker.add(onTick);
	gsap.ticker.lagSmoothing(0);

	return () => {
		gsap.ticker.remove(onTick);
		lenis?.destroy();
		lenis = null;
		ScrollTrigger.getAll().forEach((t) => t.kill());
	};
}

export function getLenis(): Lenis | null {
	return lenis;
}

export { gsap, ScrollTrigger };

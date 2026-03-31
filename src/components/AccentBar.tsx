export default function AccentBar() {
  return (
    <div style={{ position: "relative", height: 2, flexShrink: 0 }}>
      {/* Glow layer */}
      <div
        style={{
          position: "absolute",
          bottom: 0,
          left: 0,
          right: 0,
          height: 4,
          filter: "blur(3px)",
          opacity: 0.25,
          background: `linear-gradient(270deg,
            var(--cyan) 0%, var(--green) 25%,
            var(--yellow) 50%, var(--red) 75%,
            var(--cyan) 100%
          )`,
          backgroundSize: "200% 100%",
          animation: "accent-slide 4s linear infinite",
          pointerEvents: "none",
        }}
      />
      {/* Main bar */}
      <div
        style={{
          position: "absolute",
          bottom: 0,
          left: 0,
          right: 0,
          height: 1,
          opacity: 0.7,
          background: `linear-gradient(270deg,
            var(--cyan) 0%, var(--green) 25%,
            var(--yellow) 50%, var(--red) 75%,
            var(--cyan) 100%
          )`,
          backgroundSize: "200% 100%",
          animation: "accent-slide 4s linear infinite",
        }}
      />
    </div>
  );
}

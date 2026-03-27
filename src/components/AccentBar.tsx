export default function AccentBar() {
  return (
    <div
      style={{
        height: 2,
        opacity: 0.5,
        background: `linear-gradient(90deg,
          var(--cyan) 0%, var(--cyan) 22%,
          var(--green) 25%, var(--green) 47%,
          var(--yellow) 50%, var(--yellow) 72%,
          var(--red) 75%, var(--red) 100%
        )`,
      }}
    />
  );
}

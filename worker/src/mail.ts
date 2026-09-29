import type { Env } from "./env";

export async function sendRestoreCode(
  env: Env,
  email: string,
  code: string,
  mock: boolean
): Promise<void> {
  if (mock) {
    console.log(`[dev] restore code for ${email}: ${code}`);
    return;
  }

  const text = `Your Kyra code is ${code}.\n\nEnter this code in Kyra to restore Pawtrol on this Mac. It expires in 10 minutes.`;
  const html = `<p>Your Kyra code is <strong style="font-size:20px;letter-spacing:2px">${code}</strong></p><p>Enter this code in Kyra to restore Pawtrol on this Mac. It expires in 10 minutes.</p>`;

  const res = await fetch("https://api.resend.com/emails", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${env.RESEND_API_KEY}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      from: env.MAIL_FROM,
      to: [email],
      subject: `Your Kyra code: ${code}`,
      text,
      html,
    }),
  });
  if (!res.ok) throw new Error(`Resend responded ${res.status}`);
}

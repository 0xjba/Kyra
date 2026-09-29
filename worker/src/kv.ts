// Cloudflare KV rejects expirationTtl below 60 seconds.
export const KV_MIN_TTL = 60;
export const DAY = 24 * 60 * 60;

export const nowSecs = () => Math.floor(Date.now() / 1000);

export async function getJson<T>(kv: KVNamespace, key: string): Promise<T | null> {
  const raw = await kv.get(key);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}

export async function putJson(
  kv: KVNamespace,
  key: string,
  value: unknown,
  ttlSeconds?: number
): Promise<void> {
  await kv.put(
    key,
    JSON.stringify(value),
    ttlSeconds === undefined ? undefined : { expirationTtl: Math.max(KV_MIN_TTL, Math.ceil(ttlSeconds)) }
  );
}

// Fixed-window counter. KV is eventually consistent, so limits are approximate
// under concurrent bursts; they bound abuse, they are not exact quotas.
export async function rateLimit(
  kv: KVNamespace,
  key: string,
  max: number,
  windowSeconds: number
): Promise<boolean> {
  const now = nowSecs();
  const data = await getJson<{ count: number; window_start: number }>(kv, key);

  if (!data || now - data.window_start > windowSeconds) {
    await putJson(kv, key, { count: 1, window_start: now }, windowSeconds);
    return true;
  }
  if (data.count >= max) return false;

  data.count++;
  await putJson(kv, key, data, windowSeconds - (now - data.window_start));
  return true;
}

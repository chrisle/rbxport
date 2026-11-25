/** The temporary CUE target belongs to one committed track load. */
import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";

export function useCuePoint(key: string) {
  // A new token also retires A -> B -> A, even if B's metadata never arrived.
  const owner = useMemo(() => ({ key }), [key]);
  const committed = useRef<typeof owner | null>(null);
  const [point, setPoint] = useState<{ owner: typeof owner; seconds: number } | null>(null);
  useLayoutEffect(() => {
    committed.current = owner;
    return () => { if (committed.current === owner) committed.current = null; };
  }, [owner]);

  const setCuePoint = useCallback((seconds: number) => {
    if (committed.current === owner) setPoint({ owner, seconds });
  }, [owner]);
  const initialiseCuePoint = useCallback((seconds: number) => {
    if (committed.current !== owner) return;
    // A deliberate CUE/IN/call before metadata arrives already owns the target.
    setPoint(current => current?.owner === owner ? current : { owner, seconds });
  }, [owner]);
  return { cuePoint: point?.owner === owner ? point.seconds : 0, setCuePoint, initialiseCuePoint };
}

import { act } from "react";
import { getBackend } from "../src/ipc/client";

export async function installDeckWaveformFixture() {
  const backend = await getBackend();
  const fetchRows = backend.fetchRows.bind(backend);
  const waveform = backend.trackWaveform.bind(backend);
  const pcm = backend.trackPcmWaveform.bind(backend);
  const analyse = backend.analyseTrack.bind(backend);
  const ids = new Map<string, string>();
  let affectedId: string | undefined;
  let missing: Promise<Uint8Array> | undefined;
  let deliver: (() => void) | undefined;

  const fixture = {
    detailReads: [] as string[],
    analysisCalls: 0,
    omitPwv7(title: string) {
      affectedId = ids.get(title);
      if (!affectedId) throw Error(`No fixture track for ${title}`);
      missing = new Promise(resolve => { deliver = () => resolve(new Uint8Array()); });
    },
    async deliverMissingPwv7() {
      const respond = deliver;
      const response = missing;
      if (!respond || !response) throw Error("No pending detail response");
      deliver = undefined;
      const environment = globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean };
      const previous = environment.IS_REACT_ACT_ENVIRONMENT;
      environment.IS_REACT_ACT_ENVIRONMENT = true;
      try {
        // Flush both the resolved read and the resulting canvas layout effect.
        await act(async () => {
          respond();
          await response;
        });
      } finally {
        environment.IS_REACT_ACT_ENVIRONMENT = previous;
      }
    },
  };

  backend.fetchRows = async (...args) => {
    const rows = await fetchRows(...args);
    for (const row of rows) ids.set(row.title, row.id);
    return rows;
  };
  backend.trackWaveform = (id, kind, ...rest) => {
    if (id === affectedId && ["bandsDetail", "colourDetail", "monoDetail", "detail"].includes(kind)) {
      fixture.detailReads.push(kind);
      if (kind === "bandsDetail" && missing) return missing;
    }
    return waveform(id, kind, ...rest);
  };
  backend.trackPcmWaveform = (id, ...rest) => {
    if (id === affectedId) fixture.detailReads.push("pcm");
    return pcm(id, ...rest);
  };
  backend.analyseTrack = (...args) => {
    fixture.analysisCalls++;
    return analyse(...args);
  };
  return fixture;
}

export type DeckWaveformFixture = Awaited<ReturnType<typeof installDeckWaveformFixture>>;

declare global {
  interface Window {
    deckWaveformFixture: DeckWaveformFixture;
  }
}

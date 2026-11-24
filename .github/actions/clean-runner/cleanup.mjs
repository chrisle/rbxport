import { realpath, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Remove only build output and a cargo home inside this job's temp directory. */
export async function cleanRunner({ workspace, runnerTemp, cargoHome, log = console.log }) {
  for (const name of ["target", "dist", "test-results", "playwright-report", "dist-mock"]) {
    await rm(path.join(workspace, name), { recursive: true, force: true });
  }
  if (runnerTemp) {
    const cargo = path.resolve(cargoHome || path.join(runnerTemp, "cargo-home"));
    const resolveExisting = value => realpath(value).catch(error => {
      if (error.code === "ENOENT") return null;
      throw error;
    });
    const [tempReal, cargoReal] = await Promise.all([resolveExisting(runnerTemp), resolveExisting(cargo)]);
    if (tempReal && cargoReal) {
      const relative = path.relative(tempReal, cargoReal);
      if (relative && relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative)) {
        await rm(cargo, { recursive: true, force: true });
      } else {
        log("Skipped cargo home outside the job's temporary directory.");
      }
    }
  }
  log("Runner build output and per-job cargo cache cleaned after cache post steps.");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await cleanRunner({
    workspace: process.env.GITHUB_WORKSPACE || process.cwd(),
    runnerTemp: process.env.RUNNER_TEMP,
    cargoHome: process.env.CARGO_HOME,
  });
}

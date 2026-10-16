import { useEffect, useLayoutEffect, useRef } from "react";
import { registerCommands, type CommandHandler, type CommandHandlers, type CommandScope } from "@/lib/commands";
import type { Action } from "@/lib/shortcuts";

/**
 * Registers a component's commands (see `src/lib/commands.ts`) while it is
 * mounted. Each command runs the handler from the latest committed render,
 * so the handlers can read state directly; the registration itself only
 * changes when the set of commands, the scope or `enabled` does.
 */
export function useCommands(handlers: CommandHandlers, scope?: CommandScope, enabled = true): void {
  const current = useRef(handlers);
  useLayoutEffect(() => {
    current.current = handlers;
  });
  const names = Object.keys(handlers).sort().join(" ");
  useEffect(() => {
    if (!enabled || names === "") return;
    const wrapped: CommandHandlers = {};
    for (const action of names.split(" ") as Action[]) {
      const run: CommandHandler = (input) => current.current[action]?.(input) ?? false;
      wrapped[action] = run;
    }
    return registerCommands(wrapped, scope);
  }, [names, scope, enabled]);
}

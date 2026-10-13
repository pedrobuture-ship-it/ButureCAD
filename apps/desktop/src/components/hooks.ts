import { useMemo } from "react";
import { createActions } from "../actions";
import { buildCommands } from "../commands";

const actions = createActions();
const commands = buildCommands();

export function useActions() {
  return actions;
}

export function useCommands() {
  return useMemo(() => commands, []);
}

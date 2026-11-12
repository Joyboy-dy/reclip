// Ids of the jobs a browser agent (WebMCP) started in this tab. They survive a
// page reload (sessionStorage) so that restored cards still never auto-save.
const KEY = "reclip:agent-jobs";
const MAX = 200;

function read(): string[] {
  try {
    const list: unknown = JSON.parse(sessionStorage.getItem(KEY) ?? "[]");
    return Array.isArray(list) ? list.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function rememberAgentJob(id: string): void {
  try {
    const list = read().filter((x) => x !== id);
    list.push(id);
    sessionStorage.setItem(KEY, JSON.stringify(list.slice(-MAX)));
  } catch {
    // storage blocked or full: a reload then treats the card like a user card
  }
}

export const agentJobIds = (): Set<string> => new Set(read());

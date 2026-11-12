// A small JSON Schema checker for the tool inputs: the subset used by
// tools.gen.json (type, properties, required, additionalProperties, enum,
// minimum, pattern, format uri). Returns the first problem, or undefined.
export type Schema = {
  type?: string;
  properties?: Record<string, Schema>;
  required?: string[];
  additionalProperties?: boolean;
  enum?: unknown[];
  minimum?: number;
  pattern?: string;
  format?: string;
};

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

function typeError(value: unknown, type: string, path: string): string | undefined {
  const ok =
    type === "object"
      ? isObject(value)
      : type === "string"
        ? typeof value === "string"
        : type === "boolean"
          ? typeof value === "boolean"
          : type === "integer"
            ? Number.isInteger(value)
            : type === "number"
              ? typeof value === "number" && Number.isFinite(value)
              : true;
  return ok ? undefined : `"${path}" must be ${type === "integer" ? "an integer" : `a ${type}`}`;
}

export function validate(value: unknown, schema: Schema, path = "input"): string | undefined {
  const bad = schema.type ? typeError(value, schema.type, path) : undefined;
  if (bad) return bad;
  if (schema.enum && !schema.enum.includes(value)) return `"${path}" must be one of: ${schema.enum.join(", ")}`;
  if (typeof value === "number" && schema.minimum !== undefined && value < schema.minimum) {
    return `"${path}" must be at least ${schema.minimum}`;
  }
  if (typeof value === "string") {
    if (schema.pattern && !new RegExp(schema.pattern).test(value)) return `"${path}" has an invalid format`;
    if (schema.format === "uri" && !URL.canParse(value)) return `"${path}" must be a valid URL`;
  }
  if (!isObject(value)) return undefined;
  for (const key of schema.required ?? []) if (value[key] === undefined) return `"${path}.${key}" is required`;
  for (const [key, v] of Object.entries(value)) {
    const sub = schema.properties?.[key];
    if (!sub) {
      if (schema.additionalProperties === false) return `Unknown property "${path}.${key}"`;
      continue;
    }
    const err = validate(v, sub, path === "input" ? key : `${path}.${key}`);
    if (err) return err;
  }
  return undefined;
}

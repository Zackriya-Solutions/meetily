// Roles: archivos Markdown + frontmatter YAML en roles/ (SPEC §5).
import { readdirSync, readFileSync } from "node:fs";
import { basename, join } from "node:path";
import { z } from "zod";

const Category = z.string().regex(/^[a-z][a-z ]*$/, "las categorías van en inglés, en minúsculas");

export const RoleFrontmatter = z.object({
  role: z.string().min(1),
  persona: z.string().min(1),
  objective: z.string().min(1),
  gate_option: z.string().regex(/^[a-z][a-z0-9_]*$/, "gate_option va en snake_case"),
  gate_definition: z.string().min(1),
  limits: z.array(Category),
  sources: z.array(Category),
  threshold: z.number().min(0).max(1),
  calibrated_with: z.string().min(1),
  status: z.enum(["active", "experimental", "deprecated"]),
});
export type RoleFrontmatter = z.infer<typeof RoleFrontmatter>;

export interface Role extends RoleFrontmatter {
  /** Nombre del archivo sin extensión: el identificador estable del Rol. */
  id: string;
  /** Instrucciones en lenguaje natural para la Redacción. */
  instructions: string;
}

const ROLE_ID = /^[a-z][a-z0-9_]*$/;
const FRONTMATTER = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/;

export function parseRole(id: string, source: string): Role {
  if (!ROLE_ID.test(id)) throw new Error(`role id "${id}" debe ir en minúsculas y snake_case`);
  const match = FRONTMATTER.exec(source);
  if (!match) throw new Error(`roles/${id}.md: falta el frontmatter YAML`);
  const result = RoleFrontmatter.safeParse(Bun.YAML.parse(match[1]!));
  if (!result.success) {
    const issues = result.error.issues.map((i) => `${i.path.join(".")}: ${i.message}`).join("; ");
    throw new Error(`roles/${id}.md: ${issues}`);
  }
  const instructions = match[2]!.trim();
  if (!instructions) throw new Error(`roles/${id}.md: faltan las instrucciones del Rol`);
  return { ...result.data, id, instructions };
}

export function loadRoles(dir: string): Role[] {
  const roles = readdirSync(dir)
    .filter((f) => f.endsWith(".md"))
    .sort()
    .map((f) => parseRole(basename(f, ".md"), readFileSync(join(dir, f), "utf8")));

  const options = new Map<string, string>();
  for (const role of roles) {
    const other = options.get(role.gate_option);
    if (other) throw new Error(`gate_option "${role.gate_option}" repetido en ${other} y ${role.id}`);
    options.set(role.gate_option, role.id);
  }
  return roles;
}

/** Roles que entran a la Junta de una sesión: pedidos por id y con status active. */
export function selectBoard(roles: Role[], requested: string[] | undefined): Role[] {
  const active = roles.filter((r) => r.status === "active");
  if (!requested) return active;
  return active.filter((r) => requested.includes(r.id));
}

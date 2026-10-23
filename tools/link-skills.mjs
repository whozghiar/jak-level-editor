// Claude Code only reads .claude/skills/: link there every skill of the knowledge base mounted
// at .agents/skills/, including those grouped one folder down (jak-level-editor-skills/, the
// editor's own). Junctions on Windows need no admin rights; the links are ignored by git.
// `npx yarn skills:link`, after `git submodule update --init`.
import * as fs from "fs";
import * as path from "path";

const shared = path.resolve(".agents/skills");
const claude = path.resolve(".claude/skills");

const isSkill = (dir) => fs.existsSync(path.join(dir, "SKILL.md"));
const subfolders = (dir) =>
  fs
    .readdirSync(dir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && !entry.name.startsWith("."))
    .map((entry) => path.join(dir, entry.name));

const skills = new Map();
for (const dir of fs.existsSync(shared) ? subfolders(shared) : []) {
  for (const skill of isSkill(dir) ? [dir] : subfolders(dir).filter(isSkill)) {
    skills.set(path.basename(skill), skill);
  }
}
if (skills.size === 0) {
  console.error(
    "No skill in .agents/skills/: run `git submodule update --init` first",
  );
  process.exit(1);
}

fs.mkdirSync(claude, { recursive: true });
for (const name of fs.readdirSync(claude)) {
  const link = path.join(claude, name);
  if (fs.lstatSync(link).isSymbolicLink() && !skills.has(name))
    fs.unlinkSync(link);
}
for (const [name, target] of skills) {
  const link = path.join(claude, name);
  const stat = fs.lstatSync(link, { throwIfNoEntry: false });
  if (stat?.isSymbolicLink()) fs.unlinkSync(link);
  else if (stat) {
    console.warn(`${link} is a real folder, left untouched`);
    continue;
  }
  fs.symlinkSync(target, link, "junction");
}
console.log(`${skills.size} skills linked in .claude/skills/`);

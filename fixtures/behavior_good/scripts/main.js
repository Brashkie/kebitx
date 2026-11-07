// import a from "./not_here.js";
import { world } from "@minecraft/server";
import { helper } from "./lib/helper";
import "./lib/helper.js";
const s = 'import b from "./not_here_either.js"';
world.afterEvents; helper(s);

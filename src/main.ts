import { mount } from "svelte";
import App from "./App.svelte";

const target = document.getElementById("app");
export default target ? mount(App, { target }) : undefined;

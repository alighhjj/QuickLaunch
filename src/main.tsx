import { createRoot } from "react-dom/client";

import Launcher from "./views/Launcher";
import Settings from "./views/Settings";
import "./styles.css";

/** 主窗口与设置窗口共用同一个 HTML 入口，靠 query 参数分流。 */
const isSettings =
  new URLSearchParams(window.location.search).get("view") === "settings";

document.documentElement.dataset.view = isSettings ? "settings" : "launcher";

const container = document.getElementById("root");
if (!container) {
  throw new Error("找不到 #root 挂载点");
}

createRoot(container).render(isSettings ? <Settings /> : <Launcher />);

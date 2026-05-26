import "@/shared/styles/app.css";
import { mountApp } from "@/app/bootstrap/mountApp";

mountApp({ clientKind: "tauri", mobile: true });

import { Hand } from "lucide-react";
import catIcon from "../assets/cat.png";
import terminalIcon from "../assets/app-icons/xcode.png";
import keynoteIcon from "../assets/app-icons/keynote.png";
import "../styles/fda-mockup.css";

interface Props {
  granted: boolean;
}

export default function FdaMockup({ granted }: Props) {
  const rows = [
    { name: "Terminal", icon: terminalIcon, pixel: false, kyra: false, on: true },
    { name: "Kyra", icon: catIcon, pixel: true, kyra: true, on: granted },
    { name: "Keynote", icon: keynoteIcon, pixel: false, kyra: false, on: false },
  ];

  return (
    <div className="fda-mockup">
      <div className="fda-header">
        <Hand size={13} strokeWidth={2} />
        Privacy &amp; Security › Full Disk Access
      </div>
      {rows.map((r) => (
        <div
          key={r.name}
          className={`fda-row${r.kyra ? " fda-row-kyra" : ""}${r.kyra && granted ? " granted" : ""}`}
        >
          <img src={r.icon} alt="" className={`fda-app-icon${r.pixel ? " pixel" : ""}`} />
          <span className="fda-app-name">{r.name}</span>
          <span className={`fda-toggle ${r.on ? "on" : "off"}`}>
            <span className="fda-toggle-knob" />
          </span>
        </div>
      ))}
    </div>
  );
}

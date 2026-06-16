// ChatStarterGrid — welcome cards for empty state
import type { ReactElement } from "react";
import { I } from "../icons";

type StarterCard = {
  icon: ReactElement;
  tone: "blue" | "emerald" | "violet";
  title: string;
  desc: string;
  action: () => void;
};

export function ChatStarterGrid({
  onNew, onSettings,
}: {
  onNew: () => void;
  onSettings: () => void;
}): ReactElement {
  const cards: StarterCard[] = [
    {
      icon: <I.plus />,
      tone: "blue",
      title: "新建项目",
      desc: "创建新工作区，扫描代码并开始对话",
      action: onNew,
    },
    {
      icon: <I.settings />,
      tone: "violet",
      title: "配置设置",
      desc: "配置 API Key",
      action: onSettings,
    },
  ];

  return (
    <div className="welcome-grid">
      {cards.map((card) => (
        <button key={card.title} className="starter-card" onClick={card.action}>
          <div className={`starter-card-icon ${card.tone}`}>
            {card.icon}
          </div>
          <div className="starter-card-text">
            <div className="starter-card-title">{card.title}</div>
            <div className="starter-card-desc">{card.desc}</div>
          </div>
        </button>
      ))}
    </div>
  );
}

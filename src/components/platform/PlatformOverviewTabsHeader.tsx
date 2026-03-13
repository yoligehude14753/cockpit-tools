import { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Bot, Github, Layers, HelpCircle } from 'lucide-react';
import { CodexIcon } from '../icons/CodexIcon';
import { WindsurfIcon } from '../icons/WindsurfIcon';
import { KiroIcon } from '../icons/KiroIcon';
import { CursorIcon } from '../icons/CursorIcon';
import { GeminiIcon } from '../icons/GeminiIcon';
import { CodebuddyIcon } from '../icons/CodebuddyIcon';
import { QoderIcon } from '../icons/QoderIcon';

export type PlatformOverviewTab = 'overview' | 'instances';
export type PlatformOverviewHeaderId =
  | 'codex'
  | 'github-copilot'
  | 'windsurf'
  | 'kiro'
  | 'cursor'
  | 'gemini'
  | 'codebuddy'
  | 'codebuddy_cn'
  | 'qoder'
  | 'trae';

interface PlatformOverviewTabsHeaderProps {
  platform: PlatformOverviewHeaderId;
  active: PlatformOverviewTab;
  onTabChange?: (tab: PlatformOverviewTab) => void;
}

interface PlatformOverviewConfig {
  platformLabel: string;
  overviewIcon: ReactNode;
}

interface TabSpec {
  key: PlatformOverviewTab;
  label: string;
  icon: ReactNode;
}

const CONFIGS: Record<PlatformOverviewHeaderId, PlatformOverviewConfig> = {
  codex: {
    platformLabel: 'Codex',
    overviewIcon: <CodexIcon className="tab-icon" />,
  },
  'github-copilot': {
    platformLabel: 'GitHub Copilot',
    overviewIcon: <Github className="tab-icon" />,
  },
  windsurf: {
    platformLabel: 'Windsurf',
    overviewIcon: <WindsurfIcon className="tab-icon" />,
  },
  kiro: {
    platformLabel: 'Kiro',
    overviewIcon: <KiroIcon className="tab-icon" />,
  },
  cursor: {
    platformLabel: 'Cursor',
    overviewIcon: <CursorIcon className="tab-icon" />,
  },
  gemini: {
    platformLabel: 'Gemini Cli',
    overviewIcon: <GeminiIcon className="tab-icon" />,
  },
  codebuddy: {
    platformLabel: 'CodeBuddy',
    overviewIcon: <CodebuddyIcon className="tab-icon" />,
  },
  codebuddy_cn: {
    platformLabel: 'CodeBuddy CN',
    overviewIcon: <CodebuddyIcon className="tab-icon" />,
  },
  qoder: {
    platformLabel: 'Qoder',
    overviewIcon: <QoderIcon className="tab-icon" />,
  },
  trae: {
    platformLabel: 'Trae',
    overviewIcon: <Bot className="tab-icon" />,
  },
};

export function PlatformOverviewTabsHeader({
  platform,
  active,
  onTabChange,
}: PlatformOverviewTabsHeaderProps) {
  const { t } = useTranslation();
  const config = CONFIGS[platform];
  const tabs: TabSpec[] = [
    {
      key: 'overview',
      // Reuse Antigravity tab translations across platform account pages.
      label: t('overview.title', '账号总览'),
      icon: config.overviewIcon,
    },
    {
      key: 'instances',
      // Reuse Antigravity tab translations across platform account pages.
      label: t('instances.title', '多开实例'),
      icon: <Layers className="tab-icon" />,
    },
  ];

  const subtitle =
    active === 'instances'
      ? t('instances.subtitle', '多实例独立配置，多账号并行运行。')
      : t('overview.subtitle', '实时监控所有账号的配额状态。');

  return (
    <>
      <div className="page-header">
        <div className="page-title" style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          {`${config.platformLabel} ${t('settings.general.accountManagement', '账号管理')}`}
          <button
            className="btn btn-secondary icon-only"
            onClick={() => window.dispatchEvent(new CustomEvent('app-request-navigate', { detail: 'manual' }))}
            title={t('manual.navTitle', '功能使用手册')}
            style={{ padding: '6px', borderRadius: '50%', background: 'transparent', border: 'none', color: 'var(--text-secondary)' }}
          >
            <HelpCircle size={18} />
          </button>
        </div>
        <div className="page-subtitle">{subtitle}</div>
      </div>
      <div className="page-tabs-row page-tabs-center">
        <div className="page-tabs filter-tabs">
          {tabs.map((tab) => (
            <button
              key={tab.key}
              className={`filter-tab${active === tab.key ? ' active' : ''}`}
              onClick={() => onTabChange?.(tab.key)}
            >
              {tab.icon}
              <span>{tab.label}</span>
            </button>
          ))}
        </div>
      </div>
    </>
  );
}

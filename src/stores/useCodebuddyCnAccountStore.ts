import {
  CodebuddyAccount,
  getCodebuddyAccountDisplayEmail,
  getCodebuddyPlanBadge,
  getCodebuddyUsage,
} from '../types/codebuddy';
import * as codebuddyCnService from '../services/codebuddyCnService';
import { createProviderAccountStore } from './createProviderAccountStore';

const CODEBUDDY_CN_ACCOUNTS_CACHE_KEY = 'agtools.codebuddycn.accounts.cache';

export const useCodebuddyCnAccountStore = createProviderAccountStore<CodebuddyAccount>(
  CODEBUDDY_CN_ACCOUNTS_CACHE_KEY,
  {
    listAccounts: codebuddyCnService.listCodebuddyCnAccounts,
    deleteAccount: codebuddyCnService.deleteCodebuddyCnAccount,
    deleteAccounts: codebuddyCnService.deleteCodebuddyCnAccounts,
    injectAccount: codebuddyCnService.injectCodebuddyCnToVSCode,
    refreshToken: codebuddyCnService.refreshCodebuddyCnToken,
    refreshAllTokens: codebuddyCnService.refreshAllCodebuddyCnTokens,
    importFromJson: codebuddyCnService.importCodebuddyCnFromJson,
    exportAccounts: codebuddyCnService.exportCodebuddyCnAccounts,
    updateAccountTags: codebuddyCnService.updateCodebuddyCnAccountTags,
  },
  {
    getDisplayEmail: getCodebuddyAccountDisplayEmail,
    getPlanBadge: getCodebuddyPlanBadge,
    getUsage: getCodebuddyUsage,
  },
);

import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import { Account, RefreshStats } from '../types/account';
import * as accountService from '../services/accountService';
import { emitAccountsChanged, emitCurrentAccountChanged } from '../utils/accountSyncEvents';

const ACCOUNTS_STORE_KEY = 'agtools.accounts.store.v1';

// 防抖状态（在 store 外部维护，避免触发 re-render）
let fetchAccountsPromise: Promise<void> | null = null;
let fetchAccountsLastTime = 0;
let fetchCurrentPromise: Promise<void> | null = null;
let fetchCurrentLastTime = 0;
const DEBOUNCE_MS = 500;

interface AccountState {
    accounts: Account[];
    currentAccount: Account | null;
    loading: boolean;
    error: string | null;
    fetchAccounts: () => Promise<void>;
    fetchCurrentAccount: () => Promise<void>;
    addAccount: (email: string, refreshToken: string) => Promise<Account>;
    deleteAccount: (accountId: string) => Promise<void>;
    deleteAccounts: (accountIds: string[]) => Promise<void>;
    setCurrentAccount: (accountId: string) => Promise<void>;
    refreshQuota: (accountId: string) => Promise<void>;
    refreshAllQuotas: () => Promise<RefreshStats>;
    startOAuthLogin: () => Promise<Account>;
    reorderAccounts: (accountIds: string[]) => Promise<void>;
    switchAccount: (accountId: string) => Promise<Account>;
    syncCurrentFromClient: () => Promise<void>;
    updateAccountTags: (accountId: string, tags: string[]) => Promise<Account>;
}

export const useAccountStore = create<AccountState>()(
  persist(
    (set, get) => ({
      accounts: [],
      currentAccount: null,
      loading: false,
      error: null,

      fetchAccounts: async () => {
          const now = Date.now();
          
          // 如果正在请求中，且距离上次请求不足 DEBOUNCE_MS，复用现有 Promise
          if (fetchAccountsPromise && now - fetchAccountsLastTime < DEBOUNCE_MS) {
              return fetchAccountsPromise;
          }
          
          fetchAccountsLastTime = now;
          
          fetchAccountsPromise = (async () => {
              set({ loading: true, error: null });
              try {
                  const accounts = await accountService.listAccounts();
                  set({ accounts, loading: false });
              } catch (e) {
                  set({ error: String(e), loading: false });
              } finally {
                  // 请求完成后延迟清除 Promise，允许短时间内的后续调用也复用结果
                  setTimeout(() => {
                      fetchAccountsPromise = null;
                  }, 100);
              }
          })();
          
          return fetchAccountsPromise;
      },

      fetchCurrentAccount: async () => {
          const now = Date.now();
          
          // 防抖：复用正在进行的请求
          if (fetchCurrentPromise && now - fetchCurrentLastTime < DEBOUNCE_MS) {
              return fetchCurrentPromise;
          }
          
          fetchCurrentLastTime = now;
          
          fetchCurrentPromise = (async () => {
              try {
                  await accountService.syncCurrentFromClient();
                  const account = await accountService.getCurrentAccount();
                  set({ currentAccount: account });
              } catch (e) {
                  console.error('Failed to fetch current account:', e);
              } finally {
                  setTimeout(() => {
                      fetchCurrentPromise = null;
                  }, 100);
              }
          })();
          
          return fetchCurrentPromise;
      },

    addAccount: async (email: string, refreshToken: string) => {
        const account = await accountService.addAccount(email, refreshToken);
        await get().fetchAccounts();
        await emitAccountsChanged({
            platformId: 'antigravity',
            reason: 'import',
        });
        return account;
    },

    deleteAccount: async (accountId: string) => {
        const previousCurrentAccountId = get().currentAccount?.id ?? null;
        await accountService.deleteAccount(accountId);
        await get().fetchAccounts();
        await get().fetchCurrentAccount();
        await emitAccountsChanged({
            platformId: 'antigravity',
            reason: 'delete',
        });
        const nextCurrentAccountId = get().currentAccount?.id ?? null;
        if (previousCurrentAccountId !== nextCurrentAccountId) {
            await emitCurrentAccountChanged({
                platformId: 'antigravity',
                accountId: nextCurrentAccountId,
                reason: 'delete',
            });
        }
    },

    deleteAccounts: async (accountIds: string[]) => {
        const previousCurrentAccountId = get().currentAccount?.id ?? null;
        await accountService.deleteAccounts(accountIds);
        await get().fetchAccounts();
        await get().fetchCurrentAccount();
        await emitAccountsChanged({
            platformId: 'antigravity',
            reason: 'delete',
        });
        const nextCurrentAccountId = get().currentAccount?.id ?? null;
        if (previousCurrentAccountId !== nextCurrentAccountId) {
            await emitCurrentAccountChanged({
                platformId: 'antigravity',
                accountId: nextCurrentAccountId,
                reason: 'delete',
            });
        }
    },

    setCurrentAccount: async (accountId: string) => {
        await accountService.setCurrentAccount(accountId);
        await get().fetchCurrentAccount();
        await emitCurrentAccountChanged({
            platformId: 'antigravity',
            accountId: get().currentAccount?.id ?? accountId,
            reason: 'switch',
        });
    },

    refreshQuota: async (accountId: string) => {
        try {
            const updatedAccount = await accountService.fetchAccountQuota(accountId);
            // 成功：后端已更新该账号并返回最新状态（包含 quota_error），局部更新该账号，保持滚动位置不变
            set((state) => ({
                accounts: state.accounts.map((acc) =>
                    acc.id === accountId ? updatedAccount : acc
                ),
            }));
            
            // 如果刷新的是当前账号，需要同时更新 currentAccount
            const { currentAccount } = get();
            if (currentAccount?.id === accountId) {
                set({ currentAccount: updatedAccount });
            }

            // 如果后端返回了配额错误信息，需要抛出异常让 UI 捕获并显示为失败（红叉）
            if (updatedAccount.quota_error) {
                throw new Error(updatedAccount.quota_error.message);
            }
            if (updatedAccount.quota?.is_forbidden) {
                throw new Error("403 Forbidden");
            }
        } catch (e) {
            // Token 级别失败（如 invalid_grant 会改变 disabled 状态）：全量刷新确保数据正确
            // 如果是我们自己 throw 的配额错误，因为状态已经局部更新，不再需要全量刷新
            const isQuotaError = e instanceof Error && (
                get().accounts.find(a => a.id === accountId)?.quota_error?.message === e.message ||
                e.message === "403 Forbidden"
            );
            if (!isQuotaError) {
                await get().fetchAccounts();
            }
            throw e;
        } finally {
            await get().fetchCurrentAccount();
        }
    },

    refreshAllQuotas: async () => {
        const stats = await accountService.refreshAllQuotas();
        await get().fetchAccounts();
        await get().fetchCurrentAccount();
        return stats;
    },

    startOAuthLogin: async () => {
        const account = await accountService.startOAuthLogin();
        await get().fetchAccounts();
        await emitAccountsChanged({
            platformId: 'antigravity',
            reason: 'oauth',
        });
        return account;
    },

    reorderAccounts: async (accountIds: string[]) => {
        await accountService.reorderAccounts(accountIds);
        await get().fetchAccounts();
    },

    switchAccount: async (accountId: string) => {
        const account = await accountService.switchAccount(accountId);
        set({ currentAccount: account });
        await get().fetchAccounts();
        await emitCurrentAccountChanged({
            platformId: 'antigravity',
            accountId: account.id,
            reason: 'switch',
        });
        return account;
    },

    syncCurrentFromClient: async () => {
        const previousCurrentAccountId = get().currentAccount?.id ?? null;
        const result = await accountService.syncCurrentFromClient();
        if (result) {
            try {
                const account = await accountService.getCurrentAccount();
                set({ currentAccount: account });
                const nextCurrentAccountId = account?.id ?? null;
                if (previousCurrentAccountId !== nextCurrentAccountId) {
                    await emitCurrentAccountChanged({
                        platformId: 'antigravity',
                        accountId: nextCurrentAccountId,
                        reason: 'sync',
                    });
                }
            } catch (e) {
                console.error('Failed to refresh current account after client sync:', e);
            }
        }
    },

    updateAccountTags: async (accountId: string, tags: string[]) => {
        const account = await accountService.updateAccountTags(accountId, tags);
        await get().fetchAccounts();
        return account;
    },
  }),
  {
    name: ACCOUNTS_STORE_KEY,
    storage: createJSONStorage(() => localStorage),
    partialize: (state) => ({
      accounts: state.accounts,
      currentAccount: state.currentAccount,
    }),
    onRehydrateStorage: () => (state) => {
      // Migrate from old ACCOUNTS_CACHE_KEY if the new state is empty
      if (state && state.accounts.length === 0 && typeof window !== 'undefined') {
        setTimeout(() => {
          try {
            const oldAccountsRaw = localStorage.getItem('agtools.accounts.cache');
            const oldCurrentRaw = localStorage.getItem('agtools.accounts.current');
            let hasMigrated = false;
            
            if (oldAccountsRaw) {
              const oldAccounts = JSON.parse(oldAccountsRaw);
              if (Array.isArray(oldAccounts) && oldAccounts.length > 0) {
                useAccountStore.setState({ accounts: oldAccounts });
                hasMigrated = true;
              }
            }
            if (oldCurrentRaw) {
              const oldCurrent = JSON.parse(oldCurrentRaw);
              if (oldCurrent && oldCurrent.id) {
                useAccountStore.setState({ currentAccount: oldCurrent });
                hasMigrated = true;
              }
            }
            
            // Cleanup the old keys if we migrated successfully
            if (hasMigrated) {
              localStorage.removeItem('agtools.accounts.cache');
              localStorage.removeItem('agtools.accounts.current');
            }
          } catch (error) {
            // ignore migration errors
          }
        }, 0);
      }
    },
  }
));


import { create } from 'zustand';

interface UiState {
  activeTab: string;
  selectedIteration: number | null;
  setActiveTab: (tab: string) => void;
  setSelectedIteration: (iteration: number | null) => void;
}

export const useUiStore = create<UiState>()((set) => ({
  activeTab: 'overview',
  selectedIteration: null,
  setActiveTab: (tab) => set({ activeTab: tab, selectedIteration: null }),
  setSelectedIteration: (iteration) => set({ selectedIteration: iteration }),
}));

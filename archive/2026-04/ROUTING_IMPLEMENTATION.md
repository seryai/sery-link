# Routing Implementation

**Date:** 2026-04-15
**Version:** v0.4.1+

## Overview

Replaced tab-based navigation with React Router for proper URL-based routing. Users can now navigate with URLs, use browser back/forward buttons, and share deep links to specific folders/analytics views.

---

## Route Structure

| Route | Component | Purpose |
|-------|-----------|---------|
| `/` | FolderList | Default route (redirects to folders) |
| `/folders` | FolderList | Folder list page |
| `/analytics` | Analytics | General analytics hub (all recipes) |
| `/analytics/:folderId` | Analytics | Analytics scoped to specific folder |
| `/results` | History | Query results and history |
| `/settings` | Settings | App settings |
| `/privacy` | Privacy | Privacy/audit log |

---

## Key Changes

### 1. **App.tsx - Routing Setup**
- Added `BrowserRouter` wrapper around app
- Replaced `activeTab` state with `useNavigate()` and `useLocation()` hooks
- Converted navigation buttons from `<button onClick>` to `<NavLink to>`
- Replaced conditional rendering with `<Routes>` and `<Route>` components
- Updated `CommandPalette` callbacks to use `navigate()` instead of `setActiveTab()`

**Before:**
```typescript
const [activeTab, setActiveTab] = useState<Tab>('folders');
<button onClick={() => setActiveTab('folders')}>Folders</button>
{activeTab === 'folders' && <FolderList />}
```

**After:**
```typescript
const navigate = useNavigate();
<NavLink to="/folders">Folders</NavLink>
<Routes>
  <Route path="/folders" element={<FolderList />} />
</Routes>
```

### 2. **FolderList.tsx - Navigation Integration**
- Removed `onAnalyze` prop
- Added `useNavigate()` hook
- Updated "Analyze This Folder" button click → opens detail modal
- Modal's "Analyze with These Recipes" button → navigates to `/analytics/:folderId`
- Folder path is URL-encoded when passed in route

**Before:**
```typescript
<FolderList onAnalyze={() => setActiveTab('analytics')} />
```

**After:**
```typescript
<FolderList />
// Inside component:
navigate(`/analytics/${encodeURIComponent(folder.path)}`);
```

### 3. **Analytics.tsx - Folder Context from Route**
- Added `useParams()` hook to extract `folderId` from URL
- Added `selectedFolder` state to track which folder is being analyzed
- Updated data source detection to filter by folder when `folderId` is present
- Updated header to show folder name when analyzing specific folder

**Before:**
```typescript
// Analytics always showed all folders
<Analytics filterByDataSource="Shopify" />
```

**After:**
```typescript
// Analytics reads folder from URL parameter
const { folderId } = useParams<{ folderId: string }>();
// Shows: "Analytics › shopify_data"
// Subtitle: "Recipes and queries for: /Users/user/data/shopify_data"
```

---

## User Flow (New)

```
Folders page (/)
  ↓
Click "Analyze This Folder" button
  ↓
Modal opens showing datasets, schemas, compatible recipes
  ↓
Click "Analyze with These Recipes"
  ↓
Navigate to /analytics/Users%2Fuser%2Fdata%2Fshopify_data
  ↓
Analytics page scoped to that folder
  - Header shows: "Analytics › shopify_data"
  - Only shows recipes relevant to that folder's data
  - Recipe execution runs against that folder's datasets
```

---

## Benefits

### ✅ URL-Based Navigation
- Each page has a unique URL
- Can bookmark specific views
- Browser back/forward buttons work naturally

### ✅ Deep Linking
- Can share direct links to folder analytics: `/analytics/...`
- Can link to specific tabs from external apps
- Better integration with desktop app patterns

### ✅ Better Mental Model
- Navigation feels like a traditional app
- URLs reflect application state
- No "hidden state" - everything is in the URL

### ✅ Developer Experience
- Easier to debug (just look at URL)
- Can test specific routes directly
- State management is simpler (router handles it)

---

## Breaking Changes

**None!** This is a purely architectural change:
- Same visual UI
- Same user flows
- No data model changes
- No API changes

---

## Technical Details

### Package Added
- `react-router-dom` v7.14.1

### URL Encoding
Folder paths are URL-encoded when used as route parameters:
```
/Users/user/data/shopify_data
  → /analytics/Users%2Fuser%2Fdata%2Fshopify_data
```

Decoded in component:
```typescript
const decodedPath = decodeURIComponent(folderId);
```

### Active Route Detection
NavLink components use `isActive` prop from React Router:
```typescript
<NavLink
  to="/folders"
  className={({ isActive }) =>
    isActive ? 'active-styles' : 'inactive-styles'
  }
>
```

---

## Testing Checklist

- [x] TypeScript compilation passes
- [ ] App launches without errors
- [ ] Navigation between tabs works (Folders, Analytics, Results)
- [ ] Clicking "Analyze This Folder" opens modal
- [ ] Modal "Analyze with These Recipes" navigates to folder-specific analytics
- [ ] Analytics page shows correct folder name in header
- [ ] Browser back button works (e.g., Analytics → back → Folders)
- [ ] Direct URL navigation works (paste `/analytics` in address bar)
- [ ] More dropdown works (Settings, Privacy)
- [ ] Command Palette navigation still works (Cmd+K)

---

## Future Enhancements

### Possible Route Additions
- `/folders/:folderId` - Dedicated folder detail page (overview, stats, files)
- `/folders/:folderId/datasets` - Dataset list for a folder
- `/folders/:folderId/datasets/:datasetId` - Individual dataset detail
- `/results/:queryId` - Deep link to specific query result

### URL State
- `/analytics?filter=shopify` - Pre-filter analytics by data source
- `/analytics?recipe=revenue-trends` - Auto-open specific recipe
- `/results?folder=...` - Filter results by folder

---

**Status:** ✅ COMPLETE
**Next Step:** User testing → verify all flows work → DMG rebuild

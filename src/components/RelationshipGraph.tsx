// Dataset Relationship Graph — visual discovery of data connections
//
// Displays datasets as nodes and relationships as edges. Two detection methods:
// - Schema-based: FK patterns (user_id → users.id)
// - Query-based: JOIN patterns from query history
//
// Interactive:
// - Click node to highlight its connections
// - Zoom/pan to explore large graphs
// - Edge labels show join columns

import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  ReactFlow,
  Node,
  Edge,
  Controls,
  Background,
  useNodesState,
  useEdgesState,
  BackgroundVariant,
  MarkerType,
  ConnectionLineType,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { X, Loader2, Database, AlertCircle } from 'lucide-react';

interface DatasetRelationship {
  source_id: string;
  target_id: string;
  source_name: string;
  target_name: string;
  source_column: string;
  target_column: string;
  confidence: number;
  detection_method: string;
}

interface RelationshipGraphProps {
  workspaceId: string | null;
  onClose: () => void;
}

export function RelationshipGraph({ workspaceId, onClose }: RelationshipGraphProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [stats, setStats] = useState<{ datasets: number; relationships: number } | null>(null);
  const [selectedNode, setSelectedNode] = useState<string | null>(null);

  // Load relationships and build graph
  useEffect(() => {
    if (!workspaceId) {
      setError('No workspace selected');
      setLoading(false);
      return;
    }

    const loadRelationships = async () => {
      try {
        setLoading(true);
        setError(null);

        const relationships = await invoke<DatasetRelationship[]>(
          'detect_dataset_relationships',
          { workspaceId }
        );

        if (relationships.length === 0) {
          setError('No relationships found. Try running some queries with JOINs to build the graph.');
          setLoading(false);
          return;
        }

        // Build nodes from unique datasets
        const datasetMap = new Map<string, { id: string; name: string }>();
        relationships.forEach((rel) => {
          datasetMap.set(rel.source_id, { id: rel.source_id, name: rel.source_name });
          datasetMap.set(rel.target_id, { id: rel.target_id, name: rel.target_name });
        });

        // Create nodes with force-directed layout (simple grid for now)
        const nodeArray = Array.from(datasetMap.values());
        const cols = Math.ceil(Math.sqrt(nodeArray.length));
        const graphNodes: Node[] = nodeArray.map((dataset, idx) => ({
          id: dataset.id,
          type: 'default',
          position: {
            x: (idx % cols) * 250,
            y: Math.floor(idx / cols) * 150,
          },
          data: {
            label: (
              <div className="flex items-center gap-2">
                <Database className="h-4 w-4 text-purple-600 dark:text-purple-400" />
                <span className="text-sm font-medium">
                  {dataset.name.replace(/\.(parquet|csv|xlsx)$/i, '')}
                </span>
              </div>
            ),
          },
          style: {
            background: 'white',
            border: '2px solid #e2e8f0',
            borderRadius: '8px',
            padding: '10px 15px',
            fontSize: '14px',
          },
        }));

        // Create edges
        const graphEdges: Edge[] = relationships.map((rel, idx) => ({
          id: `${rel.source_id}-${rel.target_id}-${idx}`,
          source: rel.source_id,
          target: rel.target_id,
          type: ConnectionLineType.SmoothStep,
          animated: rel.confidence >= 80, // Animate high-confidence relationships
          label: `${rel.source_column} → ${rel.target_column}`,
          labelStyle: {
            fontSize: '11px',
            fill: '#64748b',
            fontWeight: 500,
          },
          labelBgStyle: {
            fill: 'white',
            fillOpacity: 0.9,
          },
          style: {
            stroke: getEdgeColor(rel.confidence, rel.detection_method),
            strokeWidth: rel.confidence >= 80 ? 2 : 1,
          },
          markerEnd: {
            type: MarkerType.ArrowClosed,
            color: getEdgeColor(rel.confidence, rel.detection_method),
          },
        }));

        setNodes(graphNodes);
        setEdges(graphEdges);
        setStats({
          datasets: datasetMap.size,
          relationships: relationships.length,
        });
        setLoading(false);
      } catch (err) {
        console.error('Failed to load relationships:', err);
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      }
    };

    loadRelationships();
  }, [workspaceId, setNodes, setEdges]);

  // Handle node click - highlight connections
  const onNodeClick = useCallback(
    (_event: React.MouseEvent, node: Node) => {
      if (selectedNode === node.id) {
        // Deselect
        setSelectedNode(null);
        setNodes((nds) =>
          nds.map((n) => ({
            ...n,
            style: { ...n.style, border: '2px solid #e2e8f0' },
          }))
        );
        setEdges((eds) =>
          eds.map((e) => ({ ...e, style: { ...e.style, opacity: 1 } }))
        );
      } else {
        // Select and highlight
        setSelectedNode(node.id);

        // Highlight connected edges
        const connectedEdges = edges.filter(
          (e) => e.source === node.id || e.target === node.id
        );
        const connectedNodeIds = new Set<string>();
        connectedEdges.forEach((e) => {
          connectedNodeIds.add(e.source);
          connectedNodeIds.add(e.target);
        });

        setNodes((nds) =>
          nds.map((n) => ({
            ...n,
            style: {
              ...n.style,
              border: connectedNodeIds.has(n.id)
                ? '2px solid #9333ea'
                : '2px solid #e2e8f0',
              opacity: connectedNodeIds.has(n.id) ? 1 : 0.3,
            },
          }))
        );

        setEdges((eds) =>
          eds.map((e) => ({
            ...e,
            style: {
              ...e.style,
              opacity: connectedEdges.some((ce) => ce.id === e.id) ? 1 : 0.1,
            },
          }))
        );
      }
    },
    [selectedNode, edges, setNodes, setEdges]
  );

  if (loading) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
        <div className="w-full max-w-4xl rounded-2xl border border-slate-200 bg-white p-8 dark:border-slate-800 dark:bg-slate-900">
          <div className="flex items-center justify-center gap-3">
            <Loader2 className="h-6 w-6 animate-spin text-purple-600" />
            <p className="text-slate-700 dark:text-slate-300">
              Analyzing dataset relationships...
            </p>
          </div>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
        <div className="w-full max-w-2xl rounded-2xl border border-slate-200 bg-white p-8 dark:border-slate-800 dark:bg-slate-900">
          <div className="flex items-start gap-3">
            <AlertCircle className="h-6 w-6 flex-shrink-0 text-amber-600 dark:text-amber-400" />
            <div className="flex-1">
              <h3 className="text-lg font-semibold text-slate-900 dark:text-slate-50">
                No Relationships Found
              </h3>
              <p className="mt-2 text-sm text-slate-600 dark:text-slate-400">{error}</p>
              <p className="mt-4 text-sm text-slate-600 dark:text-slate-400">
                Relationships are detected from:
              </p>
              <ul className="mt-2 list-inside list-disc space-y-1 text-sm text-slate-600 dark:text-slate-400">
                <li>Column names (e.g., user_id → users.id)</li>
                <li>JOIN patterns in your query history</li>
              </ul>
            </div>
          </div>
          <div className="mt-6 flex justify-end">
            <button
              onClick={onClose}
              className="rounded-lg bg-slate-100 px-4 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-200 dark:bg-slate-800 dark:text-slate-300 dark:hover:bg-slate-700"
            >
              Close
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-slate-50 dark:bg-slate-950">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div>
          <h2 className="text-lg font-semibold text-slate-900 dark:text-slate-50">
            Dataset Relationships
          </h2>
          {stats && (
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              {stats.datasets} datasets · {stats.relationships} relationships
            </p>
          )}
        </div>
        <button
          onClick={onClose}
          className="rounded-lg p-2 text-slate-600 transition-colors hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
        >
          <X className="h-5 w-5" />
        </button>
      </div>

      {/* Graph */}
      <div className="flex-1">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onNodeClick={onNodeClick}
          fitView
          minZoom={0.1}
          maxZoom={2}
          defaultEdgeOptions={{
            type: ConnectionLineType.SmoothStep,
          }}
        >
          <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
          <Controls />
        </ReactFlow>
      </div>

      {/* Legend */}
      <div className="border-t border-slate-200 bg-white px-6 py-3 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-center gap-6 text-xs text-slate-600 dark:text-slate-400">
          <div className="flex items-center gap-2">
            <div className="h-0.5 w-8 bg-purple-600" />
            <span>Query-based (high confidence)</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="h-0.5 w-8 bg-blue-600" />
            <span>Schema-based</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="h-0.5 w-8 animate-pulse bg-purple-600" />
            <span>Animated = 80%+ confidence</span>
          </div>
          <div className="ml-auto">
            <span>Click a dataset to highlight its connections</span>
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function getEdgeColor(confidence: number, detectionMethod: string): string {
  if (detectionMethod.includes('query_history')) {
    return '#9333ea'; // Purple for query-based
  }
  if (confidence >= 60) {
    return '#2563eb'; // Blue for strong schema patterns
  }
  return '#64748b'; // Gray for weak patterns
}

import { useRef, useEffect } from 'react';
import * as d3 from 'd3';
import type { Rule } from '../../types';

interface Props {
  rules: Rule[];
}

interface FlowNode {
  label: string;
  count: number;
  color: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

interface FlowLink {
  source: FlowNode;
  target: FlowNode;
  count: number;
  color: string;
}

export function RuleSankey({ rules }: Props) {
  const svgRef = useRef<SVGSVGElement>(null);

  useEffect(() => {
    if (!svgRef.current || rules.length === 0) return;

    const svg = d3.select(svgRef.current);
    svg.selectAll('*').remove();

    const width = 520;
    const height = 240;
    svg.attr('width', width).attr('height', height);

    const g = svg.append('g').attr('transform', 'translate(20,20)');
    const innerW = width - 40;
    const innerH = height - 40;

    // Count rules by status
    const provisionalCount = rules.filter((r) => r.status === 'provisional').length;
    const confirmedCount = rules.filter((r) => r.status === 'confirmed').length;
    const rejectedCount = rules.filter((r) => r.status === 'rejected').length;
    const totalCount = rules.length;

    if (totalCount === 0) return;

    // Node geometry
    const nodeW = 120;
    const nodeH = 36;

    const addedNode: FlowNode = {
      label: 'Added',
      count: totalCount,
      color: '#8CA0B4',
      x: 0,
      y: innerH / 2 - nodeH / 2,
      w: nodeW,
      h: nodeH,
    };

    const rightX = innerW - nodeW;
    const spacing = 12;
    const totalRightH = 3 * nodeH + 2 * spacing;
    const rightStartY = innerH / 2 - totalRightH / 2;

    const provisionalNode: FlowNode = {
      label: 'Provisional',
      count: provisionalCount,
      color: '#DCA03C',
      x: rightX,
      y: rightStartY,
      w: nodeW,
      h: nodeH,
    };

    const confirmedNode: FlowNode = {
      label: 'Confirmed',
      count: confirmedCount,
      color: '#509678',
      x: rightX,
      y: rightStartY + nodeH + spacing,
      w: nodeW,
      h: nodeH,
    };

    const rejectedNode: FlowNode = {
      label: 'Rejected',
      count: rejectedCount,
      color: '#FF5050',
      x: rightX,
      y: rightStartY + 2 * (nodeH + spacing),
      w: nodeW,
      h: nodeH,
    };

    const allNodes = [addedNode, provisionalNode, confirmedNode, rejectedNode];
    const links: FlowLink[] = [
      { source: addedNode, target: provisionalNode, count: provisionalCount, color: '#DCA03C' },
      { source: addedNode, target: confirmedNode, count: confirmedCount, color: '#509678' },
      { source: addedNode, target: rejectedNode, count: rejectedCount, color: '#FF5050' },
    ].filter((l) => l.count > 0);

    // Max link width proportional to count
    const maxLinkW = 18;
    const maxCount = Math.max(...links.map((l) => l.count), 1);

    // Draw links (cubic bezier curves)
    for (const link of links) {
      const linkW = Math.max((link.count / maxCount) * maxLinkW, 3);
      const sx = link.source.x + link.source.w;
      const sy = link.source.y + link.source.h / 2;
      const tx = link.target.x;
      const ty = link.target.y + link.target.h / 2;
      const mx = (sx + tx) / 2;

      g.append('path')
        .attr('d', `M${sx},${sy} C${mx},${sy} ${mx},${ty} ${tx},${ty}`)
        .attr('fill', 'none')
        .attr('stroke', link.color)
        .attr('stroke-width', linkW)
        .attr('opacity', 0.35);

      // Link label at midpoint
      g.append('text')
        .attr('x', mx)
        .attr('y', (sy + ty) / 2 - linkW / 2 - 4)
        .attr('text-anchor', 'middle')
        .attr('fill', link.color)
        .attr('font-size', '10px')
        .attr('font-family', 'ui-monospace, monospace')
        .text(String(link.count));
    }

    // Draw nodes
    for (const node of allNodes) {
      if (node.count === 0 && node !== addedNode) continue;

      g.append('rect')
        .attr('x', node.x)
        .attr('y', node.y)
        .attr('width', node.w)
        .attr('height', node.h)
        .attr('rx', 4)
        .attr('fill', node.color + '18')
        .attr('stroke', node.color)
        .attr('stroke-width', 1.5);

      g.append('text')
        .attr('x', node.x + node.w / 2)
        .attr('y', node.y + node.h / 2 - 5)
        .attr('text-anchor', 'middle')
        .attr('fill', node.color)
        .attr('font-size', '11px')
        .attr('font-weight', '600')
        .text(node.label);

      g.append('text')
        .attr('x', node.x + node.w / 2)
        .attr('y', node.y + node.h / 2 + 10)
        .attr('text-anchor', 'middle')
        .attr('fill', '#9ca3af')
        .attr('font-size', '12px')
        .attr('font-family', 'ui-monospace, monospace')
        .text(String(node.count));
    }
  }, [rules]);

  if (rules.length === 0) {
    return <p className="text-gray-500 text-sm">No rules to visualize.</p>;
  }

  return <svg ref={svgRef} />;
}

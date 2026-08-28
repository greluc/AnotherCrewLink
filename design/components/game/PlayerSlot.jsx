import React from 'react';
import { Crewmate } from './Crewmate.jsx';
import { StatusBadge } from '../feedback/StatusBadge.jsx';

/** A crewmate, their name and any state badge, in a fixed 76px slot (acl-ui SLOT).
 *  The name is under the crewmate and clipped: names run to ten characters. */
export function PlayerSlot({ name, color, shadow, size: sizeProp = 52, slot: slotProp = 76, talking = false, alive = true, badge, own = false, onClick, hat, hatBack, visor, skin, link = 'connected', usingRadio = false, assetBase, shape, overflow }) {
  const size = Number(sizeProp) || 52;
  const slot = Number(slotProp) || 76;
  return (
    <div
      onClick={onClick}
      style={{ width: own ? 96 : slot, display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2, cursor: onClick ? 'pointer' : 'default' }}
    >
      <div style={{ position: 'relative' }}>
        <Crewmate color={color} shadow={shadow} size={own ? 68 : size} talking={talking} alive={alive}
          hat={hat} hatBack={hatBack} visor={visor} skin={skin} link={link} usingRadio={usingRadio}
          {...(assetBase ? { assetBase } : {})} {...(shape ? { shape } : {})} {...(overflow ? { overflow } : {})} />
        {badge && (
          <StatusBadge state={badge} style={{ position: 'absolute', left: '50%', top: '50%', transform: 'translate(-50%,-50%)', zIndex: 10 }} />
        )}
      </div>
      <span style={{
        fontFamily: 'var(--font-ui)', fontSize: own ? 'var(--size-caption)' : 'var(--size-name-overlay)',
        maxWidth: '100%', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: 'var(--text-body)',
      }}>{name}</span>
    </div>
  );
}

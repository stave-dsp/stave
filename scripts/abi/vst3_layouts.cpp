#include "pluginterfaces/gui/iplugview.h"
#include "pluginterfaces/base/ipluginbase.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivstprocesscontext.h"
#include "pluginterfaces/vst/ivstevents.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
#include "pluginterfaces/vst/ivstunits.h"
#include <cstdio>
using namespace Steinberg; using namespace Steinberg::Vst;
#define S(T) printf("%s %zu %zu\n", #T, sizeof(T), alignof(T));
int main(){ S(ViewRect) S(PFactoryInfo) S(PClassInfo) S(PClassInfo2) S(PClassInfoW) S(BusInfo) S(RoutingInfo)
 S(ProcessSetup) S(AudioBusBuffers) S(ProcessData) S(ProcessContext) S(NoteOnEvent) S(NoteOffEvent) S(Event)
 S(ParameterInfo) S(UnitInfo) S(ProgramListInfo) }

package dev.mckayla.squill

import com.intellij.openapi.components.BaseState
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.SimplePersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.components.StoragePathMacros
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project

class SquillState : BaseState() {
	// The squill executable; empty means the one on PATH, else a
	// downloaded one.
	var path by string("")
	var highlightEmbeddedSql by property(true)
}

// Settings that follow the user from project to project.
@Service(Service.Level.APP)
@State(name = "Squill", storages = [Storage("squill.xml")])
class SquillSettings : SimplePersistentStateComponent<SquillState>(SquillState()) {
	companion object {
		fun getInstance(): SquillSettings = service()
	}
}

class SquillProjectState : BaseState() {
	var formatOnSave by property(false)
	// "Don't ask again" on the offer to turn formatOnSave on.
	var formatOnSaveDeclined by property(false)
}

// Settings for one project. Actions on save are per project in the IDE, so
// squill's is too.
@Service(Service.Level.PROJECT)
@State(name = "Squill", storages = [Storage(StoragePathMacros.WORKSPACE_FILE)])
class SquillProjectSettings :
	SimplePersistentStateComponent<SquillProjectState>(SquillProjectState()) {
	companion object {
		fun getInstance(project: Project): SquillProjectSettings = project.service()
	}
}
